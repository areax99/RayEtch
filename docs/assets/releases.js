// GitHub's /releases/latest excludes prereleases; use the list instead.
(async()=>{
const section=document.querySelector('#downloads'); if(!section)return;
const it=document.documentElement.lang==='it';
const repo='https://github.com/areax99/RayEtch';
const status=document.querySelector('#release-status');
const groups=['windows','macos','linux'];
const arch=name=>/universal/i.test(name)?(it?'Universal (Apple Silicon + Intel)':'Universal (Apple Silicon + Intel)'):/aarch64|arm64/i.test(name)?'ARM64':/x86_64|amd64|x64/i.test(name)?'x86-64':null;
try{
 const response=await fetch('https://api.github.com/repos/areax99/RayEtch/releases?per_page=20',{headers:{Accept:'application/vnd.github+json'},signal:AbortSignal.timeout(9000)});
 if(!response.ok)throw Error('HTTP '+response.status);
 const releases=await response.json();
 const release=releases.filter(r=>!r.draft).sort((a,b)=>Date.parse(b.published_at)-Date.parse(a.published_at))[0];
 if(!release||!Array.isArray(release.assets)||!release.assets.length)throw Error('No assets');
 const allowed=release.assets.filter(a=>/^https:\/\/github\.com\/areax99\/RayEtch\/releases\/download\//i.test(a.browser_download_url));
 if(!allowed.length)throw Error('No assets');
 document.querySelector('#release-name').textContent=release.name||release.tag_name;
 document.querySelector('#release-type').textContent=release.prerelease?'BETA':(it?'STABILE':'STABLE');
 const notes=document.querySelector('#release-notes');notes.href=repo+'/releases/tag/'+encodeURIComponent(release.tag_name);
 for(const group of groups){
  const target=document.querySelector('#assets-'+group);const assets=allowed.filter(a=>a.name.toLowerCase().includes('-'+group+'-')&&!/\.sha256$|\.sig$|\.txt$/i.test(a.name));
  target.replaceChildren();
  if(!assets.length){const p=document.createElement('p');p.textContent=it?'Nessun pacchetto in questa release.':'No package in this release.';target.append(p);continue;}
  assets.sort((a,b)=>Number(/universal/i.test(b.name))-Number(/universal/i.test(a.name)));
  for(const a of assets){const link=document.createElement('a');link.className='asset';link.href=a.browser_download_url;link.title=a.name;const label=document.createElement('span');label.textContent=(arch(a.name)||a.name)+(group==='linux'&&/installer/i.test(a.name)?' · Installer':'');const size=document.createElement('small');size.textContent=(a.size/1e6).toFixed(1)+' MB ↓';link.append(label,size);target.append(link);}
 }
 status.textContent=it?'Versione aggiornata tramite GitHub.':'Release information updated from GitHub.';
}catch(e){status.textContent=it?'Aggiornamento automatico non disponibile. Sono mostrati i collegamenti verificati della Beta 4; consulta tutte le release per le versioni più recenti.':'Automatic update unavailable. Verified Beta 4 links are shown; check all releases for newer builds.';}
})();
