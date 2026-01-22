

export const index = 0;
let component_cache;
export const component = async () => component_cache ??= (await import('../entries/fallbacks/layout.svelte.js')).default;
export const universal = {
  "ssr": false
};
export const universal_id = "src/routes/+layout.js";
export const imports = ["_app/immutable/nodes/0.CMWp0b1K.js","_app/immutable/chunks/DO2sOjAq.js","_app/immutable/chunks/DKHCxGQQ.js","_app/immutable/chunks/DaLeTcsY.js"];
export const stylesheets = [];
export const fonts = [];
