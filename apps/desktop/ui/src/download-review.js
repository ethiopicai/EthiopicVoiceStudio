/** @typedef {'voice-tools'|'speech'|'editing'|'recognition'|'noise'} DownloadKind */
/** Request one explicit approval; mounting a page never grants download permission.
 * @param {DownloadKind} kind @returns {Promise<string|null>} */
export function reviewDownload(kind){
 return new Promise(resolve=>window.dispatchEvent(new CustomEvent('studio-download-review',{detail:{kind,resolve}})));
}
/** Decimal units match download providers, rather than displaying GiB as GB.
 * @param {number} bytes */
export function downloadSize(bytes){
 if(!Number.isSafeInteger(bytes)||bytes<0)return 'Size unavailable';
 if(bytes>=1e9)return `${(bytes/1e9).toFixed(2)} GB`;
 if(bytes>=1e6)return `${(bytes/1e6).toFixed(1)} MB`;
 if(bytes>=1e3)return `${(bytes/1e3).toFixed(1)} KB`;
 return `${bytes} B`;
}
