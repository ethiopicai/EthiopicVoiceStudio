/** Scroll only the cards container, keeping History and the app window fixed.
 * No user IDs are interpolated into CSS selectors.
 * @param {string} id */
export function scrollToWorkspaceCard(id){
  const card=document.getElementById(id),scroller=card?.closest('[data-card-scroller]');
  if(!card||!scroller)return;
  const top=card.getBoundingClientRect().top-scroller.getBoundingClientRect().top+scroller.scrollTop-12;
  scroller.scrollTo({top:Math.max(0,top),behavior:'auto'});
}
/** @param {string} kind @param {string} id */
export function workspaceCardId(kind,id){return `voice-card-${kind}-${id}`;}
