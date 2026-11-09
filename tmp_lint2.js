const fs=require('fs');
const files=['app/lib/src/pages/hash_page.dart','app/lib/src/l10n/app_strings.dart'];
for(const f of files){
  const s=fs.readFileSync(f,'utf8');
  const stack=[];let inStr=null,esc=false,inLine=false,inBlock=false,bad=[];
  for(let i=0;i<s.length;i++){const c=s[i];
    if(inLine){if(c==='\n')inLine=false;continue;}
    if(inBlock){if(c==='*'&&s[i+1]==='/'){inBlock=false;i++;}continue;}
    if(inStr){if(esc){esc=false;continue;}if(c==='\\'){esc=true;continue;}if(c===inStr)inStr=null;continue;}
    if(c==='/'&&s[i+1]==='/'){inLine=true;i++;continue;}
    if(c==='/'&&s[i+1]==='*'){inBlock=true;i++;continue;}
    if(c==="'"||c==='"'){inStr=c;continue;}
    if(c==='{')stack.push(i);else if(c==='}'){if(!stack.length)bad.push('extra }');else stack.pop();}
  }
  console.log(f, bad.length||stack.length?('BAD '+bad.join(',')+' unclosed='+stack.length):'balanced');
}
const t=fs.readFileSync('app/lib/src/l10n/app_strings.dart','utf8').split('\n');
const zh=t.findIndex(l=>l.includes('Map<String, String> _zh'));
const en=t.findIndex(l=>l.includes('Map<String, String> _en'));
const keys=(a,b)=>t.slice(a,b).map(l=>{const m=l.match(/^\s*'([^']+)':/);return m?m[1]:null;}).filter(Boolean);
const z=keys(zh,en),e=keys(en,t.length);
console.log('zh',z.length,'en',e.length);
console.log('onlyZh',z.filter(k=>!e.includes(k)));
console.log('onlyEn',e.filter(k=>!z.includes(k)));
console.log('dupZh',z.filter((k,i)=>z.indexOf(k)!==i));