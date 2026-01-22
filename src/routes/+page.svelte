<script>
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { onMount, tick } from "svelte";
  // import Slider from "$lib/components/Slider.svelte";
  // Temporarily NOT using AppRow component to keep things raw and verifiable
  // import AppRow from "$lib/components/AppRow.svelte";

  let sysVol = 0;
  let micVol = 0;
  let brightness = 100;
  let mouseSpeed = 10;

  /** @type {Array<{pid: number, name: string, volume: number, icon_path: string}>} */
  let apps = [];

  /** @type {any} */
  let interval = undefined;
  let lastInteraction = 0;
  let isDragging = false;
  let initialLoaded = false;
  let pollingLock = false;

  async function adjustHeight() {
    await tick();
    const mainEl = document.querySelector("main");
    if (mainEl) {
      // Offset height includes border and padding
      const h = mainEl.offsetHeight;
      // Send to backend (logical pixels), small buffer to avoid scrollbar
      try {
        await invoke("resize_window", { height: h + 2 });
      } catch (e) {
        console.error(e);
      }
    }
  }

  // Monitor apps changes to resize
  $: if (initialLoaded && apps) {
    adjustHeight();
  }

  /**
   * @param {Function} func
   * @param {number} wait
   */
  function debounce(func, wait) {
    /** @type {any} */
    let timeout;
    return function (...args) {
      clearTimeout(timeout);
      timeout = setTimeout(() => func.apply(this, args), wait);
    };
  }

  // --- IPC UPDATERS ---

  /** @param {number} val */
  const updateSysVol = debounce(async (val) => {
    try {
      await invoke("set_system_volume", { vol: val / 100.0 });
    } catch (e) {
      console.error(e);
    }
  }, 50);

  /** @param {number} val */
  const updateMicVol = debounce(async (val) => {
    try {
      await invoke("set_mic_volume", { vol: val / 100.0 });
    } catch (e) {
      console.error(e);
    }
  }, 50);

  /** @param {number} val */
  const updateBrightness = debounce(async (val) => {
    try {
      await invoke("set_brightness", { val: val / 100.0 });
    } catch (e) {
      console.error(e);
    }
  }, 50);

  /** @param {number} val */
  const updateMouseSpeed = debounce(async (val) => {
    try {
      await invoke("set_mouse_speed", { val: Math.round(val) });
    } catch (e) {
      console.error(e);
    }
  }, 100);

  /**
   * @param {number} pid
   * @param {number} vol
   */
  const updateAppVol = debounce(async (pid, vol) => {
    try {
      await invoke("set_app_volume", { pid, vol: vol / 100.0 });
    } catch (e) {
      console.error(e);
    }
  }, 50);

  // --- EVENT HANDLERS ---

  function setSysVol() {
    lastInteraction = Date.now();
    updateSysVol(sysVol);
  }

  function setMicVol() {
    lastInteraction = Date.now();
    updateMicVol(micVol);
  }

  function setBrightness() {
    lastInteraction = Date.now();
    if (brightness < 10) brightness = 10;
    updateBrightness(brightness);
  }

  function setMouseSpeed() {
    lastInteraction = Date.now();
    updateMouseSpeed(mouseSpeed);
  }

  /**
   * @param {number} pid
   * @param {number} vol
   */
  function setAppVol(pid, vol) {
    lastInteraction = Date.now();
    const app = apps.find((a) => a.pid === pid);
    if (app) app.volume = vol;
    updateAppVol(pid, vol);
  }

  function handleDragStart() {
    isDragging = true;
    lastInteraction = Date.now();
  }

  function handleDragEnd() {
    isDragging = false;
    lastInteraction = Date.now();
  }

  async function loadState() {
    if (pollingLock) return;
    if (initialLoaded && Date.now() - lastInteraction < 1000) return;

    pollingLock = true;

    try {
      const results = await Promise.allSettled([
        invoke("get_system_volume"),
        invoke("get_mic_volume"),
        invoke("get_brightness"),
        invoke("get_mouse_speed"),
        invoke("get_app_volumes"),
      ]);

      if (isDragging) return;

      const [resSys, resMic, resBri, resSpd, resApps] = results;

      if (resSys.status === "fulfilled") {
        const v = resSys.value * 100;
        if (!initialLoaded || Math.abs(v - sysVol) > 1) sysVol = v;
      }

      if (resMic.status === "fulfilled") {
        const v = resMic.value * 100;
        if (!initialLoaded || Math.abs(v - micVol) > 1) micVol = v;
      }

      if (resBri.status === "fulfilled") {
        const v = resBri.value * 100;
        if (!initialLoaded || Math.abs(v - brightness) > 1) brightness = v;
      }

      if (resSpd.status === "fulfilled") {
        mouseSpeed = resSpd.value;
      }

      if (resApps.status === "fulfilled") {
        apps = resApps.value;
      }

      if (!initialLoaded) {
        initialLoaded = true;
        // Trigger initial resize after first load
        adjustHeight();
      }
    } catch (e) {
      console.error("Load State Error:", e);
    } finally {
      pollingLock = false;
    }
  }

  onMount(() => {
    loadState();
    interval = setInterval(() => {
      if (document.visibilityState === "visible") {
        loadState();
      }
    }, 2500);

    const handleGlobalUp = () => {
      if (isDragging) isDragging = false;
    };
    window.addEventListener("pointerup", handleGlobalUp);
    window.addEventListener("blur", handleGlobalUp);
    // Resize on window resize (system scale change)? Typically just on logic change.

    return () => {
      if (interval) clearInterval(interval);
      window.removeEventListener("pointerup", handleGlobalUp);
      window.removeEventListener("blur", handleGlobalUp);
    };
  });
</script>

<main>
  <section class="merged-controls">
    <div class="control-row">
      <div class="icon-box" title="System Volume">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          ><path d="M11 5L6 9H2v6h4l5 4V5z" /><path
            d="M15.54 8.46a5 5 0 0 1 0 7.07"
          /><path d="M19.07 4.93a10 10 0 0 1 0 14.14" /></svg
        >
      </div>
      <div class="slider-container">
        <input
          type="range"
          min="0"
          max="100"
          bind:value={sysVol}
          on:input={setSysVol}
          on:pointerdown={handleDragStart}
          on:pointerup={handleDragEnd}
        />
        <span class="value-badge">{Math.round(sysVol)}</span>
      </div>
    </div>

    <div class="control-row">
      <div class="icon-box" title="Microphone">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          ><path
            d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"
          /><path d="M19 10v2a7 7 0 0 1-14 0v-2" /><line
            x1="12"
            y1="19"
            x2="12"
            y2="22"
          /><line x1="8" y1="22" x2="16" y2="22" /></svg
        >
      </div>
      <div class="slider-container">
        <input
          type="range"
          min="0"
          max="100"
          bind:value={micVol}
          on:input={setMicVol}
          on:pointerdown={handleDragStart}
          on:pointerup={handleDragEnd}
        />
        <span class="value-badge">{Math.round(micVol)}</span>
      </div>
    </div>

    <div class="control-row">
      <div class="icon-box" title="Brightness">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          ><circle cx="12" cy="12" r="4" /><path d="M12 2v2" /><path
            d="M12 20v2"
          /><path d="M4.93 4.93l1.41 1.41" /><path
            d="M17.66 17.66l1.41 1.41"
          /><path d="M2 12h2" /><path d="M20 12h2" /><path
            d="M4.93 19.07l1.41-1.41"
          /><path d="M17.66 6.34l1.41-1.41" /></svg
        >
      </div>
      <div class="slider-container">
        <input
          type="range"
          min="10"
          max="100"
          bind:value={brightness}
          on:input={setBrightness}
          on:pointerdown={handleDragStart}
          on:pointerup={handleDragEnd}
        />
        <span class="value-badge">{Math.round(brightness)}</span>
      </div>
    </div>

    <div class="control-row">
      <div class="icon-box" title="Mouse Speed">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          ><rect x="5" y="2" width="14" height="20" rx="7" /><path
            d="M12 6v4"
          /></svg
        >
      </div>
      <div class="slider-container">
        <input
          type="range"
          min="1"
          max="20"
          bind:value={mouseSpeed}
          on:input={setMouseSpeed}
          on:pointerdown={handleDragStart}
          on:pointerup={handleDragEnd}
        />
        <span class="value-badge">{mouseSpeed}</span>
      </div>
    </div>
  </section>

  <section class="app-section">
    <div class="app-list">
      {#each apps as app (app.pid + app.name)}
        <div class="app-row">
          <div class="icon-box" title={app.name}>
            {#if app.icon_path}
              <img
                class="app-icon"
                src={app.icon_path.startsWith("data:")
                  ? app.icon_path
                  : convertFileSrc(app.icon_path)}
                on:error={(e) => {
                  const target = /** @type {HTMLImageElement} */ (
                    e.currentTarget
                  );
                  target.style.display = "none";
                  if (target.nextElementSibling) {
                    /** @type {HTMLElement} */ (
                      target.nextElementSibling
                    ).style.display = "flex";
                  }
                }}
                alt=""
              />
            {/if}
            <div
              class="app-icon-fallback"
              style="display: {!app.icon_path || app.icon_path === ''
                ? 'flex'
                : 'none'}"
            >
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                ><rect x="2" y="3" width="20" height="14" rx="2" ry="2"
                ></rect><line x1="8" y1="21" x2="16" y2="21"></line><line
                  x1="12"
                  y1="17"
                  x2="12"
                  y2="21"
                ></line></svg
              >
            </div>
          </div>

          <div class="slider-container">
            <input
              type="range"
              min="0"
              max="100"
              value={Math.round(app.volume * 100)}
              on:input={(e) =>
                setAppVol(app.pid, e.currentTarget.valueAsNumber)}
              on:pointerdown={handleDragStart}
              on:pointerup={handleDragEnd}
            />
            <span class="value-badge">{Math.round(app.volume * 100)}</span>
          </div>
        </div>
      {:else}
        <div class="loading">Scanning sessions...</div>
      {/each}
    </div>
  </section>
</main>

<style>
  :global(body) {
    font-family: "Segoe UI", system-ui, sans-serif;
    background: transparent !important;
    color: #333;
    margin: 0;
    padding: 0;
    user-select: none;
    overflow: hidden;
  }

  main {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    height: auto; /* Allow auto height for adaptation */
    box-sizing: border-box;
    /* overflow-y: hidden;  Hide scrollbar, let window resize handle it */
    overflow: hidden;

    background: rgba(255, 255, 255, 0.45);
    backdrop-filter: blur(32px) saturate(180%);
    -webkit-backdrop-filter: blur(32px) saturate(180%);

    /* Removed thick border to blend better, used subtle shadow/outline */
    /* border: 1px solid rgba(255, 255, 255, 0.6); */
    border-radius: 12px; /* Match inner content radius closer */
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.1);
  }

  /* Remove scrollbar styles as we don't expect scrolling now */
  main::-webkit-scrollbar {
    width: 0px;
  }

  section {
    background: #ffffff;
    padding: 12px;
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05); /* Softer shadow */
    flex-shrink: 0;
  }

  .control-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .icon-box {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #3b82f6;
  }

  .slider-container {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  input[type="range"] {
    flex: 1;
    appearance: none;
    height: 4px;
    background: #e5e5e5;
    border-radius: 2px;
    outline: none;
    cursor: pointer;
  }

  input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    background: #3b82f6;
    border-radius: 50%;
    box-shadow: 0 1px 3px rgba(59, 130, 246, 0.4);
    transition: transform 0.1s;
  }

  input[type="range"]::-webkit-slider-thumb:hover {
    transform: scale(1.1);
  }

  .value-badge {
    min-width: 24px;
    text-align: right;
    font-size: 0.85em;
    color: #888;
    font-feature-settings: "tnum";
  }

  .app-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .app-row {
    padding: 6px 8px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 12px;
    transition: background-color 0.2s;
  }

  .app-row:hover {
    background: rgba(0, 0, 0, 0.04);
  }

  .app-icon {
    width: 20px;
    height: 20px;
    object-fit: contain;
    border-radius: 5px;
    background: rgba(0, 0, 0, 0.03);
    padding: 2px;
    image-rendering: -webkit-optimize-contrast;
    image-rendering: high-quality;
  }

  .app-icon-fallback {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #999;
    background: #f5f5f5;
    border-radius: 5px;
  }

  /* Override section margin or padding if needed for merged controls */
  .merged-controls {
    gap: 16px; /* Slightly more space between items in the large group */
  }

  .loading {
    text-align: center;
    padding: 20px;
    color: #999;
    font-size: 0.9em;
    font-style: italic;
  }
</style>
