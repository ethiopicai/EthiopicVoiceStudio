// Empty sandbox grants no script, same-origin, form, top-navigation, or popup privileges.
export const HOME_FRAME_SANDBOX = '';
export const HOME_FRAME_ALLOW = "camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'; clipboard-write 'none'";
export const HOME_FRAME_CSP = "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; media-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
/** Wrap trusted bundled HTML only. This is defense in depth, NOT a remote HTML sanitizer.
 * The native backend must sanitize remote/cache content and inject its own leading CSP.
 * @param {string} html */
export function bundledHomeDocument(html) {
  return `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${HOME_FRAME_CSP}"><meta name="referrer" content="no-referrer"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"></head><body>${html}</body></html>`;
}

/** Apply only bundled palette CSS to a document already wrapped/sanitized by the native service.
 * Its leading CSP and the iframe sandbox remain unchanged; no scripts or links are added.
 * @param {string} html @param {'light'|'dark'} theme @param {string} palette */
export function themedHomeDocument(html,theme,palette){
  const mode=theme==='light'?'light':'dark';
  return html.replace(/<html(?=[\s>])/i,`<html data-theme="${mode}"`)
    .replace(/<\/head>/i,`<style id="studio-palette">${palette}</style></head>`);
}
