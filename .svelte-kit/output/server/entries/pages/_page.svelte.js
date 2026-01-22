import { w as attr, x as bind_props, y as ensure_array_like } from "../../chunks/index.js";
import { invoke } from "@tauri-apps/api/core";
import { l as ssr_context, m as fallback, k as escape_html } from "../../chunks/context.js";
import "clsx";
function onDestroy(fn) {
  /** @type {SSRContext} */
  ssr_context.r.on_destroy(fn);
}
function Slider($$renderer, $$props) {
  let value = fallback($$props["value"], 0);
  let min = fallback($$props["min"], 0);
  let max = fallback($$props["max"], 100);
  let step = fallback($$props["step"], 1);
  let icon = fallback($$props["icon"], "");
  $$renderer.push(`<div class="slider-container svelte-oyl6e3">`);
  if (icon) {
    $$renderer.push("<!--[-->");
    $$renderer.push(`<span class="icon svelte-oyl6e3">${escape_html(icon)}</span>`);
  } else {
    $$renderer.push("<!--[!-->");
  }
  $$renderer.push(`<!--]--> <input type="range"${attr("min", min)}${attr("max", max)}${attr("step", step)}${attr("value", value)} class="slider svelte-oyl6e3"/> <span class="value svelte-oyl6e3">${escape_html(Math.round(value))}</span></div>`);
  bind_props($$props, { value, min, max, step, icon });
}
function AppRow($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    let name = $$props["name"];
    let pid = $$props["pid"];
    let volume = fallback($$props["volume"], 1);
    let onVolumeChange = $$props["onVolumeChange"];
    let sliderVal = Math.round(volume * 100);
    sliderVal = Math.round(volume * 100);
    let $$settled = true;
    let $$inner_renderer;
    function $$render_inner($$renderer3) {
      $$renderer3.push(`<div class="app-row svelte-uwjq8b"><div class="name svelte-uwjq8b"${attr("title", name)}>${escape_html(name)}</div> `);
      Slider($$renderer3, {
        max: 100,
        get value() {
          return sliderVal;
        },
        set value($$value) {
          sliderVal = $$value;
          $$settled = false;
        }
      });
      $$renderer3.push(`<!----></div>`);
    }
    do {
      $$settled = true;
      $$inner_renderer = $$renderer2.copy();
      $$render_inner($$inner_renderer);
    } while (!$$settled);
    $$renderer2.subsume($$inner_renderer);
    bind_props($$props, { name, pid, volume, onVolumeChange });
  });
}
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    let sysVol = 0;
    let micVol = 0;
    let brightness = 100;
    let mouseSpeed = 10;
    let apps = [];
    async function setAppVol(pid, vol) {
      await invoke("set_app_volume", { pid, vol });
    }
    onDestroy(() => {
    });
    let $$settled = true;
    let $$inner_renderer;
    function $$render_inner($$renderer3) {
      $$renderer3.push(`<main class="control-center svelte-1uha8ag"><div class="header svelte-1uha8ag"><h2 class="svelte-1uha8ag">Control Center</h2> <button class="refresh-btn svelte-1uha8ag">↻</button></div> <section class="svelte-1uha8ag"><h3 class="svelte-1uha8ag">System</h3> <div class="control-group svelte-1uha8ag"><label class="svelte-1uha8ag">Volume</label> `);
      Slider($$renderer3, {
        max: 100,
        get value() {
          return sysVol;
        },
        set value($$value) {
          sysVol = $$value;
          $$settled = false;
        }
      });
      $$renderer3.push(`<!----></div> <div class="control-group svelte-1uha8ag"><label class="svelte-1uha8ag">Mic</label> `);
      Slider($$renderer3, {
        max: 100,
        get value() {
          return micVol;
        },
        set value($$value) {
          micVol = $$value;
          $$settled = false;
        }
      });
      $$renderer3.push(`<!----></div></section> <section class="svelte-1uha8ag"><h3 class="svelte-1uha8ag">Display &amp; Input</h3> <div class="control-group svelte-1uha8ag"><label class="svelte-1uha8ag">Brightness</label> `);
      Slider($$renderer3, {
        max: 100,
        get value() {
          return brightness;
        },
        set value($$value) {
          brightness = $$value;
          $$settled = false;
        }
      });
      $$renderer3.push(`<!----></div> <div class="control-group svelte-1uha8ag"><label class="svelte-1uha8ag">Sensitivity</label> `);
      Slider($$renderer3, {
        min: 1,
        max: 20,
        get value() {
          return mouseSpeed;
        },
        set value($$value) {
          mouseSpeed = $$value;
          $$settled = false;
        }
      });
      $$renderer3.push(`<!----></div></section> <section class="apps-section svelte-1uha8ag"><h3 class="svelte-1uha8ag">Apps Mixer</h3> <div class="app-list svelte-1uha8ag"><!--[-->`);
      const each_array = ensure_array_like(apps);
      for (let $$index = 0, $$length = each_array.length; $$index < $$length; $$index++) {
        let app = each_array[$$index];
        AppRow($$renderer3, {
          name: app.name,
          pid: app.pid,
          volume: app.volume,
          onVolumeChange: setAppVol
        });
      }
      $$renderer3.push(`<!--]--></div></section></main>`);
    }
    do {
      $$settled = true;
      $$inner_renderer = $$renderer2.copy();
      $$render_inner($$inner_renderer);
    } while (!$$settled);
    $$renderer2.subsume($$inner_renderer);
  });
}
export {
  _page as default
};
