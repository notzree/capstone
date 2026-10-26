use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use csi_host::star::{Command, StarController, StartOptions};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    path::PathBuf,
};
#[derive(Args)]
pub struct StarArgs {
    #[arg(long, default_value = "auto")]
    device: String,
    /// Newline JSON commands/replies/events; prompts and diagnostics stay on stderr.
    #[arg(long)]
    json: bool,
}
#[derive(Parser)]
struct Line {
    #[command(subcommand)]
    command: Input,
}
#[derive(Subcommand)]
enum Input {
    Nodes,
    Status,
    Discover {
        #[arg(long, default_value_t = 10)]
        seconds: u32,
    },
    Start(StartArgs),
    Stop,
    Begin {
        activity: String,
        #[arg(long)]
        position: Option<String>,
    },
    End,
    Note {
        text: String,
    },
    Quit,
}
#[derive(Args)]
struct StartArgs {
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long)]
    geometry: Option<PathBuf>,
    #[arg(long)]
    plan: Option<PathBuf>,
    #[arg(long, default_value_t = 10)]
    rate: u16,
    #[arg(long, default_value_t = 3600)]
    duration: u32,
    #[arg(long)]
    allow_long_run: bool,
    #[arg(long, value_delimiter = ',')]
    nodes: Vec<String>,
}
fn parse(line: &str, json_mode: bool) -> Result<(Value, Command)> {
    if json_mode {
        let mut value: Value = serde_json::from_str(line)?;
        let id = value
            .as_object_mut()
            .and_then(|o| o.remove("id"))
            .unwrap_or(Value::Null);
        return Ok((id, serde_json::from_value(value)?));
    }
    // REPL arguments support ordinary shell-style single/double quoting and escapes.
    let words = shell_words(line)?;
    let input = Line::try_parse_from(std::iter::once("csi-star".to_owned()).chain(words))?.command;
    let command = match input {
        Input::Nodes => Command::Nodes,
        Input::Status => Command::Status,
        Input::Discover { seconds } => {
            if seconds != 10 {
                bail!("v1 discovery is exactly ten seconds")
            }
            Command::Discover
        }
        Input::Start(a) => Command::Start(StartOptions {
            out: a.out,
            socket: a.socket,
            geometry: a.geometry,
            plan: a.plan,
            rate: a.rate,
            duration: a.duration,
            allow_long_run: a.allow_long_run,
            nodes: a.nodes,
        }),
        Input::Stop => Command::Stop,
        Input::Begin { activity, position } => Command::Begin { activity, position },
        Input::End => Command::End,
        Input::Note { text } => Command::Note { text },
        Input::Quit => Command::Shutdown,
    };
    Ok((Value::Null, command))
}
fn shell_words(line: &str) -> Result<Vec<String>> {
    let mut out = vec![];
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            word.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if Some(c) == quote {
            quote = None;
            continue;
        }
        if quote.is_none() && (c == '\'' || c == '"') {
            quote = Some(c);
            continue;
        }
        if quote.is_none() && c.is_whitespace() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word))
            }
        } else {
            word.push(c)
        }
    }
    if quote.is_some() || escaped {
        bail!("unterminated quote/escape")
    }
    if !word.is_empty() {
        out.push(word)
    }
    Ok(out)
}
pub fn run(args: StarArgs) -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async move{
    let controller=StarController::open(&args.device).await?;let mut events=controller.events();
    let (tx,mut lines)=tokio::sync::mpsc::channel(16);
    std::thread::spawn(move||{for line in io::stdin().lock().lines(){if tx.blocking_send(line).is_err(){break}}});
    if !args.json{eprintln!("Star RX attached. Commands: nodes, status, discover, start --out DIR / --socket PATH, begin ACTIVITY, end, stop, quit");}
    let mut active=tokio::task::JoinSet::new();let mut ending=false;
    #[cfg(unix)]let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop{tokio::select!{
        line=lines.recv(),if !ending=>{match line{Some(Ok(line)) if !line.trim().is_empty()=>{match parse(&line,args.json){Ok((id,command))=>{let shutdown=matches!(command,Command::Shutdown);ending|=shutdown;let c=controller.clone();active.spawn(async move{(id,c.request(command).await,shutdown)});},Err(e)=>println!("{}",json!({"type":"reply","ok":false,"error":e.to_string()}))}},Some(Ok(_))=>{},_=>{ending=true;let c=controller.clone();active.spawn(async move{(Value::Null,c.request(Command::Shutdown).await,true)});}}},
        event=events.recv()=>{match event{Ok(value)=>{
            if args.json { println!("{}",json!({"type":"event","event":value})); }
            else if value["event"]=="device" {
                let packet=&value["packet"];let fields=&packet["fields"];
                match packet["kind"].as_str() {
                    Some("RunState")=>eprintln!("RX {}",fields["state"].as_str().unwrap_or("unknown")),
                    Some("DiscoveryComplete")=>eprintln!("Discovery complete: {} TXs",fields["count"]),
                    Some("RosterSnapshot") if fields["total"].as_u64().unwrap_or(0)>0=>eprintln!("Discovered TX {} (epoch {})",fields["node"]["id"].as_str().unwrap_or("unknown"),fields["node"]["epoch"]),
                    _=>{}
                }
            } else if value["event"]=="outputs_ready" && value["connect_primary_now"]==true { eprintln!("Outputs ready; connect Python to acknowledge the Arrow schema."); }
            else if value["event"]=="run_finished" { eprintln!("Run closed; radio STOP confirmed: {}",value["status"]["stop_confirmed"]); }
        },Err(tokio::sync::broadcast::error::RecvError::Lagged(n))=>eprintln!("control event reader missed {n} events; query status"),Err(_)=>break}},
        result=active.join_next(),if !active.is_empty()=>{if let Some(Ok((id,result,shutdown)))=result{match result{Ok(value)=>println!("{}",json!({"type":"reply","id":id,"ok":true,"result":value})),Err(e)=>println!("{}",json!({"type":"reply","id":id,"ok":false,"error":e.to_string()}))}if shutdown{break}}},
        _=tokio::signal::ctrl_c(),if !ending=>{ending=true;let c=controller.clone();active.spawn(async move{(Value::Null,c.request(Command::Shutdown).await,true)});},
        _=term.recv(),if !ending=>{ending=true;let c=controller.clone();active.spawn(async move{(Value::Null,c.request(Command::Shutdown).await,true)});}
    }}Ok(())
})
}
