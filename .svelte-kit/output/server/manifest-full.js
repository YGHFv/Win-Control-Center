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
		client: {start:"_app/immutable/entry/start.BKwutBhx.js",app:"_app/immutable/entry/app.CTnaDVFz.js",imports:["_app/immutable/entry/start.BKwutBhx.js","_app/immutable/chunks/CdC4UQCJ.js","_app/immutable/chunks/CGmmsghJ.js","_app/immutable/chunks/DSc4XEBz.js","_app/immutable/entry/app.CTnaDVFz.js","_app/immutable/chunks/CGmmsghJ.js","_app/immutable/chunks/BG-g19ID.js","_app/immutable/chunks/Di5biFW1.js","_app/immutable/chunks/DSc4XEBz.js","_app/immutable/chunks/hk1Sb3z8.js","_app/immutable/chunks/DDZbv4JG.js"],stylesheets:[],fonts:[],uses_env_dynamic_public:false},
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
