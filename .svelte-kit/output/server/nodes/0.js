

export const index = 0;
let component_cache;
export const component = async () => component_cache ??= (await import('../entries/fallbacks/layout.svelte.js')).default;
export const universal = {
  "ssr": false
};
export const universal_id = "src/routes/+layout.js";
export const imports = ["_app/immutable/nodes/0.Bn0KnFqP.js","_app/immutable/chunks/D-U0E7Jf.js","_app/immutable/chunks/NcJHILLz.js","_app/immutable/chunks/mOqeCVMl.js"];
export const stylesheets = [];
export const fonts = [];
