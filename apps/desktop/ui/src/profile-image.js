export const DEFAULT_PROFILE_IMAGES=[
  {id:'male',label:'Male',src:'/profile-images/default-male.jpg'},
  {id:'female',label:'Female',src:'/profile-images/default-female.jpg'}
];

const TYPES=new Set(['image/jpeg','image/png','image/webp']);
/** Make a small local thumbnail; canvas export also drops the photo's metadata.
 * @param {File} file @returns {Promise<string>} */
export async function profileImageFromFile(file){
  if(!TYPES.has(file.type))throw Error('Choose a JPG, PNG or WebP image.');
  if(!file.size||file.size>5*1024*1024)throw Error('Choose an image smaller than 5 MB.');
  // The desktop CSP permits data images, not blob image URLs. Keep decoding
  // local without broadening the app's security policy.
  /** @type {string} */
  const source=await new Promise((resolve,reject)=>{
    const reader=new FileReader();reader.onload=()=>resolve(String(reader.result));
    reader.onerror=()=>reject(Error('This image could not be read. Try selecting it again.'));
    reader.onabort=()=>reject(Error('Image selection was interrupted. Try again.'));
    reader.readAsDataURL(file);
  });
  const image=new Image();image.src=source;
  try{await image.decode();}catch{throw Error('This image could not be opened. Try another photo.');}
  if(!image.naturalWidth||!image.naturalHeight||image.naturalWidth*image.naturalHeight>24_000_000)throw Error('Choose an image up to 24 megapixels.');
  const canvas=document.createElement('canvas');canvas.width=256;canvas.height=256;
  const context=canvas.getContext('2d');if(!context)throw Error('The profile image could not be prepared. Try again.');
  const side=Math.min(image.naturalWidth,image.naturalHeight);
  context.fillStyle='#e7efeb';context.fillRect(0,0,256,256);
  context.drawImage(image,(image.naturalWidth-side)/2,(image.naturalHeight-side)/2,side,side,0,0,256,256);
  return canvas.toDataURL('image/jpeg',.85);
}
