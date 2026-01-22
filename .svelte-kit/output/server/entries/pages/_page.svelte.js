import { w as attr, x as ensure_array_like, y as attr_class, z as stringify, F as attr_style } from "../../chunks/index.js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { e as escape_html } from "../../chunks/context.js";
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    let sysVol = 0;
    let micVol = 0;
    let brightness = 100;
    let mouseSpeed = 10;
    let apps = [];
    $$renderer2.push(`<main class="svelte-1uha8ag"><section class="merged-controls svelte-1uha8ag"><div class="control-row svelte-1uha8ag"><div class="icon-box svelte-1uha8ag" title="System Volume"><svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5L6 9H2v6h4l5 4V5z"></path><path d="M15.54 8.46a5 5 0 0 1 0 7.07"></path><path d="M19.07 4.93a10 10 0 0 1 0 14.14"></path></svg></div> <div class="slider-container svelte-1uha8ag"><input type="range" min="0" max="100"${attr(
      "value",
      /**
       * @param {Function} func
       * @param {number} wait
       */
      /** @type {any} */
      // --- IPC UPDATERS ---
      /** @param {number} val */
      /** @param {number} val */
      /** @param {number} val */
      /** @param {number} val */
      /**
       * @param {number} pid
       * @param {number} vol
       */
      // --- EVENT HANDLERS ---
      /**
       * @param {number} pid
       * @param {number} vol
       */
      // Force Svelte 5 compatibility refresh
      /**
       * @param {number} pid
       * @param {boolean} currentMute
       */
      // Force Svelte 5 compatibility refresh
      // Don't poll for 3 seconds after any interaction to give OS time to settle and prevent flicker
      // Merge instead of replacing to preserve local state of what's currently being interacted with
      // Resize on window resize (system scale change)? Typically just on logic change.
      sysVol
    )} class="svelte-1uha8ag"/> <span class="value-badge svelte-1uha8ag">${escape_html(Math.round(sysVol))}</span></div></div> <div class="control-row svelte-1uha8ag"><div class="icon-box svelte-1uha8ag" title="Microphone"><svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"></path><path d="M19 10v2a7 7 0 0 1-14 0v-2"></path><line x1="12" y1="19" x2="12" y2="22"></line><line x1="8" y1="22" x2="16" y2="22"></line></svg></div> <div class="slider-container svelte-1uha8ag"><input type="range" min="0" max="100"${attr("value", micVol)} class="svelte-1uha8ag"/> <span class="value-badge svelte-1uha8ag">${escape_html(Math.round(micVol))}</span></div></div> <div class="control-row svelte-1uha8ag"><div class="icon-box svelte-1uha8ag" title="Brightness"><svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"></circle><path d="M12 2v2"></path><path d="M12 20v2"></path><path d="M4.93 4.93l1.41 1.41"></path><path d="M17.66 17.66l1.41 1.41"></path><path d="M2 12h2"></path><path d="M20 12h2"></path><path d="M4.93 19.07l1.41-1.41"></path><path d="M17.66 6.34l1.41-1.41"></path></svg></div> <div class="slider-container svelte-1uha8ag"><input type="range" min="0" max="100"${attr("value", brightness)} class="svelte-1uha8ag"/> <span class="value-badge svelte-1uha8ag">${escape_html(Math.round(brightness))}</span></div></div> <div class="control-row svelte-1uha8ag"><div class="icon-box svelte-1uha8ag" title="Mouse Speed"><svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="2" width="14" height="20" rx="7"></rect><path d="M12 6v4"></path></svg></div> <div class="slider-container svelte-1uha8ag"><input type="range" min="1" max="20"${attr("value", mouseSpeed)} class="svelte-1uha8ag"/> <span class="value-badge svelte-1uha8ag">${escape_html(mouseSpeed)}</span></div></div></section> <section class="app-section svelte-1uha8ag"><div class="app-list svelte-1uha8ag">`);
    const each_array = ensure_array_like(apps);
    if (each_array.length !== 0) {
      $$renderer2.push("<!--[-->");
      for (let $$index = 0, $$length = each_array.length; $$index < $$length; $$index++) {
        let app = each_array[$$index];
        $$renderer2.push(`<div class="app-row svelte-1uha8ag"><div${attr_class(`icon-box ${stringify(app.is_muted ? "muted" : "")}`, "svelte-1uha8ag")}${attr("title", app.name)} style="cursor: pointer;">`);
        if (app.icon_path && app.icon_path !== "") {
          $$renderer2.push("<!--[-->");
          $$renderer2.push(`<img class="app-icon svelte-1uha8ag"${attr_style(`filter: ${stringify(app.is_muted ? "grayscale(1) opacity(0.5)" : "none")}`)}${attr("src", app.icon_path.startsWith("data:") ? app.icon_path : convertFileSrc(app.icon_path))} alt="" onerror="this.__e=event"/>`);
        } else {
          $$renderer2.push("<!--[!-->");
          $$renderer2.push(`<div class="app-icon-fallback svelte-1uha8ag"><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect><line x1="8" y1="21" x2="16" y2="21"></line><line x1="12" y1="17" x2="12" y2="21"></line></svg></div>`);
        }
        $$renderer2.push(`<!--]--></div> <div class="slider-container svelte-1uha8ag"><input type="range" min="0" max="100"${attr("value", app.volume_display)} class="svelte-1uha8ag"/> <span class="value-badge svelte-1uha8ag">${escape_html(app.volume_display)}</span></div></div>`);
      }
    } else {
      $$renderer2.push("<!--[!-->");
      $$renderer2.push(`<div class="loading svelte-1uha8ag">Scanning sessions...</div>`);
    }
    $$renderer2.push(`<!--]--></div></section></main>`);
  });
}
export {
  _page as default
};
