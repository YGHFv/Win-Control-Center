export const manifest = (() => {
function __memo(fn) {
	let value;
	return () => value ??= (value = fn());
}

return {
	appDir: "_app",
	appPath: "_app",
	assets: new Set(["favicon.png","svelte.svg","tauri.svg","vite.svg"]),
	mimeTypes: {".png":"image/png",".svg":"image/svg+xml"},
	_: {
		client: {start:"_app/immutable/entry/start.CfSVslOi.js",app:"_app/immutable/entry/app.RQKaiFow.js",imports:["_app/immutable/entry/start.CfSVslOi.js","_app/immutable/chunks/Bj4szm_k.js","_app/immutable/chunks/NcJHILLz.js","_app/immutable/chunks/DoS_njpE.js","_app/immutable/entry/app.RQKaiFow.js","_app/immutable/chunks/NcJHILLz.js","_app/immutable/chunks/DEJiXCbA.js","_app/immutable/chunks/D-U0E7Jf.js","_app/immutable/chunks/DoS_njpE.js","_app/immutable/chunks/Dgf7cJfp.js","_app/immutable/chunks/mOqeCVMl.js"],stylesheets:[],fonts:[],uses_env_dynamic_public:false},
		nodes: [
			__memo(() => import('./nodes/0.js')),
			__memo(() => import('./nodes/1.js')),
			__memo(() => import('./nodes/2.js'))
		],
		remotes: {
			
		},
		routes: [
			{
				id: "/",
				pattern: /^\/$/,
				params: [],
				page: { layouts: [0,], errors: [1,], leaf: 2 },
				endpoint: null
			}
		],
		prerendered_routes: new Set([]),
		matchers: async () => {
			
			return {  };
		},
		server_assets: {}
	}
}
})();
