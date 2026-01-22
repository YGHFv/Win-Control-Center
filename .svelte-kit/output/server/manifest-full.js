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
		client: {start:"_app/immutable/entry/start.Df6l7gkY.js",app:"_app/immutable/entry/app.BswVfade.js",imports:["_app/immutable/entry/start.Df6l7gkY.js","_app/immutable/chunks/G0fV9zhJ.js","_app/immutable/chunks/DKHCxGQQ.js","_app/immutable/chunks/DbDpVIfj.js","_app/immutable/entry/app.BswVfade.js","_app/immutable/chunks/DKHCxGQQ.js","_app/immutable/chunks/HKu4UYYu.js","_app/immutable/chunks/DO2sOjAq.js","_app/immutable/chunks/DbDpVIfj.js","_app/immutable/chunks/MbY572J-.js","_app/immutable/chunks/DaLeTcsY.js"],stylesheets:[],fonts:[],uses_env_dynamic_public:false},
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
