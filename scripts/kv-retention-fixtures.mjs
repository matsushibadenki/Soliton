// Synthetic fixed retention workload; no user conversations or model downloads.
import assert from 'node:assert/strict';import {writeFile} from 'node:fs/promises';import {pathToFileURL} from 'node:url';
import {cacheHistory} from './cache-memory-workload.mjs';import {cases} from './benchmark-engine-gateway.mjs';
export function retentionFixtures(){
 const short=cases().filter(c=>c.phase==='new-input'),rows=[];assert.equal(short.length,3);
 for(const c of short)rows.push({...c,label:'prime-'+c.language,phase:'prime',expected:c.exact});
 for(const i of [0,1,2,0,1,2,0,1,2])rows.push({...cacheHistory(i),label:`history-${rows.length}`,phase:rows.length<6?'seed':'revisit',expected:'STRATA-7391',maxTokens:64});
 for(const c of short)rows.push({...c,label:'short-'+c.language,phase:'short',expected:c.exact});
 for(const c of cases().filter(c=>c.phase==='decode'))rows.push({...c,expected:null});
 assert.equal(rows.length,18);return rows;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){assert.equal(process.argv.length,3,'Supply a new output path');await writeFile(process.argv[2],JSON.stringify(retentionFixtures()),{flag:'wx',mode:0o600});}
