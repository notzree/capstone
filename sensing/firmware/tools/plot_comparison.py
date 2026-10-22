#!/usr/bin/env python3
"""Plot individual pilot repetitions; no confidence-interval or pooled-quantile claim."""
import argparse
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

p=argparse.ArgumentParser();p.add_argument('--comparison',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();data=json.loads(a.comparison.read_text())
labels=['Star A→B','Star B→A','Independent TX/RX','Packet-driven turns'];modes=['star-a','star-b','independent','pair']
fig,axes=plt.subplots(2,3,figsize=(13,7),layout='constrained')
colors={'a0':'#2563eb','b0':'#d97706'}
for row,target in enumerate([10,50]):
 for trial in data['trials']:
  if trial['target_hz']!=target:continue
  x=modes.index(trial['mode'])
  for link in trial['links']:
   tx=link['direction'][:2];offset=(-.11 if tx=='a0' else .11)+(-.025 if trial['repeat']==1 else .025)
   vals=[link['rate_hz'],100*link['interior_missing_fraction'],link['interarrival_node_us']['p99']/1000]
   for col,v in enumerate(vals):axes[row,col].scatter(x+offset,v,c=colors[tx],marker='o' if trial['repeat']==1 else '^',s=42,alpha=.85,zorder=3)
 for col,ylabel in enumerate(['Received CSI / s','Missing probe interiors (%)','Per-run p99 capture gap (ms)']):
  ax=axes[row,col];ax.set_xticks(range(4),labels,rotation=20,ha='right');ax.set_ylabel(ylabel);ax.grid(axis='y',alpha=.25);ax.set_title(f'{target} Hz requested per transmitter');ax.spines[['top','right']].set_visible(False)
 axes[row,0].axhline(target,ls=':',c='#64748b',lw=1)
fig.suptitle('Two ESP32s, ~70 cm desk spacing — two 45 s repeats per condition',fontsize=14)
from matplotlib.lines import Line2D
handles=[Line2D([],[],marker='o',color='none',markerfacecolor=colors['a0'],label='A→B'),Line2D([],[],marker='o',color='none',markerfacecolor=colors['b0'],label='B→A'),Line2D([],[],marker='o',color='#64748b',ls='none',label='Repeat 1'),Line2D([],[],marker='^',color='#64748b',ls='none',label='Repeat 2')]
fig.legend(handles=handles,loc='outside lower center',ncol=4,frameon=False,title='Descriptive pilot; RF conditions varied. Pair replies are request-driven; see actual TX rates in report.')
a.out.parent.mkdir(parents=True,exist_ok=True);fig.savefig(a.out,dpi=180);fig.savefig(a.out.with_suffix('.svg'));print(a.out)
