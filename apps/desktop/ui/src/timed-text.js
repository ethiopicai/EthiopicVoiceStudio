/** @param {ReadonlyArray<{start:number,end:number}>} tokens */
export function validTimedSequence(tokens) {
  let last=0;
  for(const token of tokens){
    if(!Number.isFinite(token.start)||!Number.isFinite(token.end)||token.start<last||token.end<token.start)return false;
    last=token.end;
  }
  return true;
}

/** Binary lookup for ordered, nonoverlapping intervals. Boundaries are [start,end).
 * Validate externally supplied sequences once when data changes, not per frame.
 * @param {ReadonlyArray<{start:number,end:number}>} tokens @param {number} seconds
 * @returns {number|null}
 */
export function timedTokenIndexAt(tokens,seconds) {
  if(!Number.isFinite(seconds)||seconds<0)return null;
  let low=0,high=tokens.length-1,candidate=-1;
  while(low<=high){
    const middle=Math.floor((low+high)/2),start=tokens[middle].start;
    if(!Number.isFinite(start))return null;
    if(start<=seconds){candidate=middle;low=middle+1;}else high=middle-1;
  }
  if(candidate<0)return null;
  const token=tokens[candidate];
  return token.start>=0&&Number.isFinite(token.end)&&token.end>token.start&&seconds<token.end?candidate:null;
}
