use std::{collections::{BTreeMap,BTreeSet},path::Path};
use csi_host::{session::SessionReader,RecordKind};
use csi_protocol::{frame,Message};
fn main(){
 let dir=std::env::args().nth(1).unwrap();
 let reader=SessionReader::open(Path::new(&dir)).unwrap();
 let mut layouts=BTreeMap::<String,u64>::new();
 let mut raw=BTreeSet::new();
 let mut errors=Vec::new();
 let mut csi=0;
 for item in reader {
  let record=item.unwrap();
  if record.kind!=RecordKind::Frame {errors.push(serde_json::json!({"mono_ns":record.mono_ns,"kind":format!("{:?}",record.kind),"payload":record.payload}));continue}
  if let (_,Message::Csi(c))=frame::decode(&record.payload).unwrap(){
   csi+=1;
   let key=format!("sig_mode={} mcs={} cwb={} sgi={} stbc={} channel={} len={} first_word_invalid={} gain={:?}",c.rx.sig_mode,c.rx.mcs,c.rx.cwb,c.rx.sgi,c.rx.stbc,c.rx.channel,c.data.len(),c.first_word_invalid,c.rx.gain);
   *layouts.entry(key).or_default()+=1;
   raw.insert(c.data.to_vec());
  }
 }
 println!("{}",serde_json::to_string_pretty(&serde_json::json!({"csi_records":csi,"layouts":layouts,"distinct_raw_csi":raw.len(),"stream_errors":errors})).unwrap());
}
