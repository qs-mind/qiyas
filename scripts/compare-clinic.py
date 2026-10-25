"""Strict quantities/records/results comparison, rebinding only runtime path/session tokens."""
import json, pathlib, struct, sys, zipfile
import xml.etree.ElementTree as ET
before, after = map(lambda p:pathlib.Path(p).resolve(),sys.argv[1:3])

def canonical(value, root):
 if isinstance(value,str):
  value = value.replace('/crates/qiyas-ai/../..','')
  for root in sorted({str(before), str(after)}, key=len, reverse=True):
   value = value.replace(root, '$RUN')
  if '\nrevision: ' in value:
   caption, token = value.rsplit('\nrevision: ',1)
   try: return caption+'\nrevision: '+json.dumps(canonical(json.loads(token),root),sort_keys=True)
   except json.JSONDecodeError: pass
  return value
 if isinstance(value,list): return [canonical(v,root) for v in value]
 if isinstance(value,dict):
  result={k:canonical(v,root) for k,v in value.items()}
  if {'session','document_revision','knowledge_revision','context'} <= value.keys():
   # Session UUID is random. Run the after replay with CLINIC_SESSION_DIR pointing at
   # the baseline directory; context fingerprints and revision counts remain exact.
   result['session']='$SESSION'
  return result
 return value

def compare(a,b,path,errors):
 if type(a)!=type(b): errors.append((path,a,b)); return
 if isinstance(a,dict):
  for key in sorted(a.keys()|b.keys()):
   if key not in a or key not in b: errors.append((path+'/'+key,a.get(key),b.get(key)))
   else: compare(a[key],b[key],path+'/'+key,errors)
 elif isinstance(a,list):
  if len(a)!=len(b): errors.append((path+'/length',len(a),len(b)))
  for i,(x,y) in enumerate(zip(a,b)): compare(x,y,path+'/'+str(i),errors)
 elif isinstance(a,float):
  if struct.pack('>d',a)!=struct.pack('>d',b): errors.append((path,a,b))
 elif a!=b: errors.append((path,a,b))

files={p.name for p in before.glob('*-state.json')}|{p.name for p in before.glob('*-result.json')}|{'final-groups.json'}
summary={'before':str(before),'after':str(after),'files':len(files),'mismatches':{}}
for name in sorted(files):
 if not (after/name).exists(): summary['mismatches'][name]={'missing':True}; continue
 errors=[]
 compare(canonical(json.loads((before/name).read_text()),before),canonical(json.loads((after/name).read_text()),after),'',errors)
 if errors: summary['mismatches'][name]={'count':len(errors),'first':errors[:20]}
images = sorted(before.glob('[0-9][0-9][0-9].png'))
summary['images'] = len(images)
for name in sorted({p.name for p in after.glob('[0-9][0-9][0-9].png')} - {p.name for p in images}):
 summary['mismatches'][name]={'unexpected_image':True}
for image in images:
 other = after/image.name
 if not other.exists(): summary['mismatches'][image.name]={'missing':True}
 elif image.read_bytes()!=other.read_bytes(): summary['mismatches'][image.name]={'png_bytes_differ':True}
# Exported quantities are compared too. Only workbook creation/modification timestamps
# are volatile; sheet XML, formulas, styles and PDF bytes must match exactly.
exports = sorted([*before.glob('*.pdf'), *before.glob('*.xlsx')])
summary['exports'] = len(exports)
for export in exports:
 other = after/export.name
 if not other.exists(): summary['mismatches'][export.name]={'missing':True}; continue
 if export.suffix == '.pdf':
  if export.read_bytes()!=other.read_bytes(): summary['mismatches'][export.name]={'pdf_bytes_differ':True}
  continue
 with zipfile.ZipFile(export) as a, zipfile.ZipFile(other) as b:
  different=[]
  for name in sorted(set(a.namelist()) | set(b.namelist())):
   if name not in a.namelist() or name not in b.namelist(): different.append(name); continue
   left,right=a.read(name),b.read(name)
   if name == 'docProps/core.xml':
    def metadata(data):
     root=ET.fromstring(data)
     for kind in ['created','modified']:
      for value in root.findall('{http://purl.org/dc/terms/}'+kind): value.text='$TIMESTAMP'
     return ET.tostring(root)
    left,right=metadata(left),metadata(right)
   if left!=right: different.append(name)
  if different: summary['mismatches'][export.name]={'members_differ':different}
rows=lambda root:[json.loads(line) for line in (root/'calls.jsonl').read_text().splitlines()]
a,b=rows(before),rows(after)
summary.update(before_calls=len(a),after_calls=len(b),before_seconds=sum(r['seconds'] for r in a),after_seconds=sum(r['seconds'] for r in b),after_max_seconds=max(r['seconds'] for r in b))
summary['speedup']=summary['before_seconds']/summary['after_seconds']
summary['debug_over_3s']=[{'index':r['index'],'op':r['op'],'seconds':r['seconds']} for r in b if r['seconds']>=3]
summary['release_over_1s']=[{'index':r['index'],'op':r['op'],'seconds':r['seconds']} for r in b if r['seconds']>=1]
path=after/'comparison.json';path.write_text(json.dumps(summary,indent=2))
print(json.dumps({k:v for k,v in summary.items() if k!='mismatches'},indent=2)); print('mismatched files:',len(summary['mismatches']),str(path))
sys.exit(bool(summary['mismatches']) or len(a)!=len(b))
