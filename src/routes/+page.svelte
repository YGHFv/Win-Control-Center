<script>
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  // import Slider from "$lib/components/Slider.svelte";
  // Temporarily NOT using AppRow component to keep things raw and verifiable
  // import AppRow from "$lib/components/AppRow.svelte";

  /** @typedef {{ id: string, name: string, is_default: boolean }} TrayMenuDevice */
  /** @typedef {{ speaker: boolean, microphone: boolean, brightness: boolean, mouse_speed: boolean, volume_mixer: boolean }} PanelVisibility */
  /** @typedef {{ playback_devices: TrayMenuDevice[], recording_devices: TrayMenuDevice[], autostart: boolean, blur_style: string, controls: PanelVisibility }} TrayMenuState */
  /** @typedef {{ id: string, number: number, name: string, brightness: number | null, brightness_display: number }} MonitorBrightness */
  /** @typedef {{ pending: number | undefined, running: boolean }} BrightnessUpdate */

  let sysVol = 0;
  let sysMuted = false;
  let micVol = 0;
  let micMuted = false;
  /** @type {MonitorBrightness[]} */
  let monitors = [];
  /** @type {Map<string, BrightnessUpdate>} */
  const brightnessUpdates = new Map();
  /** @type {Record<string, string>} */
  let brightnessErrors = {};
  let monitorsLoaded = false;
  let monitorLoadError = "";
  let monitorPollingLock = false;
  let mouseSpeed = 10;
  let isTrayMenu = false;
  let activeSubmenu = "";
  let trayMenuExpanded = false;

  /** @type {PanelVisibility} */
  let panelVisibility = {
    speaker: true,
    microphone: true,
    brightness: true,
    mouse_speed: true,
    volume_mixer: true,
  };

  /** @type {Array<{ id: string, label: string }>} */
  const styleOptions = [
    { id: "mica", label: "云母 (Mica)" },
    { id: "mica_alt", label: "云母 Alt (Mica Alt)" },
    { id: "acrylic", label: "亚克力 (Acrylic)" },
    { id: "blur", label: "模糊 (Blur)" },
  ];

  /** @type {Array<{ id: keyof PanelVisibility, label: string }>} */
  const controlOptions = [
    { id: "speaker", label: "扬声器" },
    { id: "microphone", label: "麦克风" },
    { id: "brightness", label: "屏幕亮度" },
    { id: "mouse_speed", label: "鼠标灵敏度" },
    { id: "volume_mixer", label: "音量合成器" },
  ];

  /** @type {TrayMenuState} */
  let trayMenuState = {
    playback_devices: [],
    recording_devices: [],
    autostart: false,
    blur_style: "mica_alt",
    controls: panelVisibility,
  };

  /** @type {Array<{pid: number, name: string, volume: number, is_muted: boolean, volume_display: number, icon_path: string}>} */
  let apps = [];

  /** @type {any} */
  let interval = undefined;
  let lastInteraction = 0;
  let isDragging = false;
  let initialLoaded = false;
  let pollingLock = false;
  $: hasMergedControls =
    panelVisibility.speaker ||
    panelVisibility.microphone ||
    panelVisibility.brightness ||
    panelVisibility.mouse_speed;

  async function adjustHeight() {
    await tick();
    const mainEl = document.querySelector("main");
    if (mainEl) {
      // Offset height is the most reliable for border-box
      const h = mainEl.offsetHeight;
      try {
        await invoke("resize_window", { height: h });
      } catch (e) {
        console.error(e);
      }
    }
  }

  // Monitor apps changes to resize
  $: if (initialLoaded && apps) {
    adjustHeight();
  }
  $: if (initialLoaded && monitors) {
    adjustHeight();
  }
  $: if (initialLoaded && brightnessErrors) {
    adjustHeight();
  }

  /**
   * @template {(...args: any[]) => any} T
   * @param {T} func
   * @param {number} wait
   * @returns {T}
   */
  function debounce(func, wait) {
    /** @type {any} */
    let timeout;
    return /** @type {T} */ ((...args) => {
      clearTimeout(timeout);
      timeout = setTimeout(() => func(...args), wait);
    });
  }

  // --- IPC UPDATERS ---

  /** @param {number} val */
  const updateSysVol = debounce(async (/** @type {number} */ val) => {
    try {
      await invoke("set_system_volume", { vol: val / 100.0 });
    } catch (e) {
      console.error(e);
    }
  }, 50);

  /** @param {number} val */
  const updateMicVol = debounce(async (/** @type {number} */ val) => {
    try {
      await invoke("set_mic_volume", { vol: val / 100.0 });
    } catch (e) {
      console.error(e);
    }
  }, 50);

  /**
   * @param {string} id
   * @param {number} val
   */
  function updateBrightness(id, val) {
    let update = brightnessUpdates.get(id);
    if (!update) {
      update = { pending: undefined, running: false };
      brightnessUpdates.set(id, update);
    }
    update.pending = val;
    if (!update.running) void flushBrightness(id);
  }

  /** @param {string} id */
  async function flushBrightness(id) {
    const update = brightnessUpdates.get(id);
    if (!update || update.running) return;
    update.running = true;
    try {
      // Keep only the latest queued value during a slow hardware write.
      while (update.pending !== undefined) {
        const val = update.pending;
        update.pending = undefined;
        try {
          await invoke("set_monitor_brightness", { id, val: val / 100.0 });
          delete brightnessErrors[id];
          brightnessErrors = { ...brightnessErrors };
        } catch (e) {
          brightnessErrors = { ...brightnessErrors, [id]: String(e) };
          console.error("Monitor Brightness Error:", e);
        }
      }
    } finally {
      update.running = false;
      brightnessUpdates.delete(id);
      lastInteraction = Date.now();
    }
  }

  /** @param {number} val */
  const updateMouseSpeed = debounce(async (/** @type {number} */ val) => {
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
  const updateAppVol = debounce(async (
    /** @type {number} */ pid,
    /** @type {number} */ vol,
  ) => {
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

  /**
   * @param {MonitorBrightness} monitor
   * @param {number} value
   */
  function setBrightness(monitor, value) {
    lastInteraction = Date.now();
    monitor.brightness_display = value;
    monitors = monitors;
    updateBrightness(monitor.id, value);
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
    if (app) {
      app.volume_display = vol;
      app.volume = vol / 100.0;
      apps = apps; // Force Svelte 5 compatibility refresh
    }
    updateAppVol(pid, vol);
  }

  /**
   * @param {number} pid
   * @param {boolean} currentMute
   */
  async function toggleAppMute(pid, currentMute) {
    lastInteraction = Date.now();
    const app = apps.find((a) => a.pid === pid);
    if (app) {
      app.is_muted = !currentMute;
      apps = apps; // Force Svelte 5 compatibility refresh
    }
    try {
      await invoke("set_app_mute", { pid, mute: !currentMute });
    } catch (e) {
      console.error(e);
    }
  }

  function handleDragStart() {
    isDragging = true;
    lastInteraction = Date.now();
  }

  function handleDragEnd() {
    isDragging = false;
    lastInteraction = Date.now();
  }

  async function toggleSysMute() {
    lastInteraction = Date.now();
    sysMuted = !sysMuted;
    try {
      await invoke("set_system_mute", { mute: sysMuted });
    } catch (e) {
      console.error(e);
    }
  }

  async function toggleMicMute() {
    lastInteraction = Date.now();
    micMuted = !micMuted;
    try {
      await invoke("set_mic_mute", { mute: micMuted });
    } catch (e) {
      console.error(e);
    }
  }

  async function loadState() {
    if (pollingLock) return;
    // Don't poll for 3 seconds after any interaction to give OS time to settle and prevent flicker
    if (initialLoaded && Date.now() - lastInteraction < 3000) return;

    pollingLock = true;

    try {
      const results = await Promise.allSettled([
        invoke("get_system_volume"),
        invoke("get_mic_volume"),
        invoke("get_mouse_speed"),
        invoke("get_app_volumes"),
      ]);

      if (isDragging) return;

      const [resSys, resMic, resSpd, resApps] = results;

      if (resSys.status === "fulfilled") {
        const [v, m] = resSys.value;
        const vol = v * 100;
        if (!initialLoaded || Math.abs(vol - sysVol) > 1 || sysMuted !== m) {
          sysVol = vol;
          sysMuted = m;
        }
      }

      if (resMic.status === "fulfilled") {
        const [v, m] = resMic.value;
        const vol = v * 100;
        if (!initialLoaded || Math.abs(vol - micVol) > 1 || micMuted !== m) {
          micVol = vol;
          micMuted = m;
        }
      }

      if (resSpd.status === "fulfilled") {
        mouseSpeed = resSpd.value;
      }

      if (resApps.status === "fulfilled") {
        const newApps = resApps.value.map((/** @type {any} */ a) => ({
          ...a,
          volume_display: Math.round(a.volume * 100),
        }));
        // Merge instead of replacing to preserve local state of what's currently being interacted with
        if (!isDragging) {
          apps = newApps;
        }
      }

      if (!initialLoaded) {
        initialLoaded = true;
        adjustHeight();
      }
    } catch (e) {
      console.error("Load State Error:", e);
    } finally {
      pollingLock = false;
    }
  }

  async function loadMonitors() {
    if (monitorPollingLock || isDragging) return;
    if (monitorsLoaded && Date.now() - lastInteraction < 3000) return;
    monitorPollingLock = true;
    const interactionAtStart = lastInteraction;
    try {
      const results = /** @type {Array<Omit<MonitorBrightness, "brightness_display">>} */ (
        await invoke("get_monitor_brightness")
      );
      if (isDragging || interactionAtStart !== lastInteraction) return;
      monitors = results.map((monitor) => {
        const previous = monitors.find((item) => item.id === monitor.id);
        const update = brightnessUpdates.get(monitor.id);
        return {
          ...monitor,
          brightness_display:
            previous && (update?.running || update?.pending !== undefined)
              ? previous.brightness_display
              : Math.round((monitor.brightness ?? 0) * 100),
        };
      });
      monitorLoadError = "";
      monitorsLoaded = true;
    } catch (e) {
      monitorLoadError = "无法读取显示器，请稍后重试";
      console.error("Monitor List Error:", e);
    } finally {
      monitorPollingLock = false;
    }
  }

  async function loadTrayMenuState() {
    try {
      trayMenuState = /** @type {TrayMenuState} */ (
        await invoke("get_tray_menu_state")
      );
    } catch (e) {
      console.error("Tray Menu State Error:", e);
    }
  }

  async function loadPanelVisibility() {
    try {
      panelVisibility = /** @type {PanelVisibility} */ (
        await invoke("get_panel_visibility")
      );
      await adjustHeight();
    } catch (e) {
      console.error("Panel Visibility Error:", e);
    }
  }

  /**
   * @param {string} menu
   */
  function setActiveSubmenu(menu) {
    if (activeSubmenu === menu) return;
    activeSubmenu = menu;
    const expanded = menu !== "";
    trayMenuExpanded = expanded;
    invoke("set_tray_menu_expanded", {
      expanded,
      submenu: expanded ? menu : null,
    }).catch((e) => {
      console.error("Tray Menu Layout Error:", e);
    });
  }

  function resetTrayMenuLayout() {
    if (!activeSubmenu && !trayMenuExpanded) return;
    activeSubmenu = "";
    trayMenuExpanded = false;
    invoke("set_tray_menu_expanded", {
      expanded: false,
      submenu: null,
    }).catch((e) => {
      console.error("Tray Menu Layout Error:", e);
    });
  }

  /**
   * @param {string} action
   * @param {string | undefined} value
   */
  async function runTrayMenuAction(action, value = undefined) {
    try {
      await invoke("tray_menu_action", { action, value: value ?? null });
      if (action !== "quit") {
        await loadTrayMenuState();
        if (action === "control") {
          panelVisibility = trayMenuState.controls;
        }
      }
    } catch (e) {
      console.error("Tray Menu Action Error:", e);
    }
  }

  async function hideTrayMenu() {
    try {
      resetTrayMenuLayout();
      await invoke("hide_tray_menu");
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    isTrayMenu =
      window.location.hash === "#menu" ||
      new URLSearchParams(window.location.search).get("view") === "menu";

    if (isTrayMenu) {
      const handleTrayMenuFocus = () => {
        resetTrayMenuLayout();
        loadTrayMenuState();
      };
      handleTrayMenuFocus();
      window.addEventListener("focus", handleTrayMenuFocus);

      return () => {
        window.removeEventListener("focus", handleTrayMenuFocus);
      };
    }

    loadPanelVisibility();
    loadState();
    loadMonitors();
    interval = setInterval(() => {
      loadState();
      loadMonitors();
    }, 2500);

    /** @type {undefined | (() => void)} */
    let unlistenPanelVisibility;
    listen("panel-visibility-changed", (event) => {
      panelVisibility = /** @type {PanelVisibility} */ (event.payload);
      adjustHeight();
    }).then((unlisten) => {
      unlistenPanelVisibility = unlisten;
    });

    const handleGlobalUp = () => {
      if (isDragging) isDragging = false;
    };
    window.addEventListener("pointerup", handleGlobalUp);
    window.addEventListener("blur", handleGlobalUp);
    // Resize on window resize (system scale change)? Typically just on logic change.

    return () => {
      if (interval) clearInterval(interval);
      for (const update of brightnessUpdates.values()) {
        update.pending = undefined;
      }
      if (unlistenPanelVisibility) unlistenPanelVisibility();
      window.removeEventListener("pointerup", handleGlobalUp);
      window.removeEventListener("blur", handleGlobalUp);
    };
  });
</script>

{#if isTrayMenu}
  <div
    class="tray-menu-stage {activeSubmenu ? 'expanded' : ''} {activeSubmenu}"
    role="presentation"
    oncontextmenu={(e) => {
      e.preventDefault();
      hideTrayMenu();
    }}
    onmouseleave={() => {
      setActiveSubmenu("");
    }}
  >
    <nav class="context-menu" aria-label="Tray menu">
      <button
        class="menu-row {activeSubmenu === 'controls' ? 'active' : ''}"
        onmouseenter={() => {
          setActiveSubmenu("controls");
        }}
      >
        <span class="menu-icon">
          <svg viewBox="0 0 24 24"><path d="M4 7h10" /><path d="M18 7h2" /><path d="M16 5v4" /><path d="M4 17h2" /><path d="M10 17h10" /><path d="M8 15v4" /></svg>
        </span>
        <span class="menu-label">控制选项</span>
        <span class="menu-chevron">›</span>
      </button>

      <button
        class="menu-row {activeSubmenu === 'playback' ? 'active' : ''}"
        onmouseenter={() => {
          setActiveSubmenu("playback");
        }}
      >
        <span class="menu-icon">
          <svg viewBox="0 0 24 24"><path d="M11 5 6 9H3v6h3l5 4V5Z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /></svg>
        </span>
        <span class="menu-label">播放设备</span>
        <span class="menu-chevron">›</span>
      </button>

      <button
        class="menu-row {activeSubmenu === 'recording' ? 'active' : ''}"
        onmouseenter={() => {
          setActiveSubmenu("recording");
        }}
      >
        <span class="menu-icon">
          <svg viewBox="0 0 24 24"><path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3Z" /><path d="M19 11a7 7 0 0 1-14 0" /><path d="M12 18v3" /></svg>
        </span>
        <span class="menu-label">录音设备</span>
        <span class="menu-chevron">›</span>
      </button>

      <button
        class="menu-row {activeSubmenu === 'style' ? 'active' : ''}"
        onmouseenter={() => {
          setActiveSubmenu("style");
        }}
      >
        <span class="menu-icon">
          <svg viewBox="0 0 24 24"><path d="M4 8h16" /><path d="M8 4v16" /><path d="M16 4v16" /><path d="M4 16h16" /></svg>
        </span>
        <span class="menu-label">模糊样式</span>
        <span class="menu-chevron">›</span>
      </button>

      <div class="menu-separator"></div>

      <button
        class="menu-row"
        onmouseenter={() => {
          setActiveSubmenu("");
        }}
        onclick={() => runTrayMenuAction("autostart")}
      >
        <span class="menu-check">{trayMenuState.autostart ? "✓" : ""}</span>
        <span class="menu-label">开机自启</span>
      </button>

      <button
        class="menu-row"
        onmouseenter={() => {
          setActiveSubmenu("");
        }}
        onclick={() => runTrayMenuAction("quit")}
      >
        <span class="menu-icon">
          <svg viewBox="0 0 24 24"><path d="M12 3v9" /><path d="M6.4 6.4a8 8 0 1 0 11.2 0" /></svg>
        </span>
        <span class="menu-label">退出</span>
      </button>
    </nav>

    {#if activeSubmenu}
      <div class="context-submenu {activeSubmenu}">
        {#if activeSubmenu === "controls"}
          {#each controlOptions as control}
            <button
              class="menu-row"
              onclick={() => runTrayMenuAction("control", control.id)}
            >
              <span class="menu-check">{trayMenuState.controls[control.id] ? "✓" : ""}</span>
              <span class="menu-label">{control.label}</span>
            </button>
          {/each}
        {:else if activeSubmenu === "playback"}
          {#each trayMenuState.playback_devices as device}
            <button
              class="menu-row"
              title={device.name}
              onclick={() => runTrayMenuAction("playback", device.id)}
            >
              <span class="menu-check">{device.is_default ? "✓" : ""}</span>
              <span class="menu-label">{device.name}</span>
            </button>
          {:else}
            <div class="menu-empty">没有可用播放设备</div>
          {/each}
        {:else if activeSubmenu === "recording"}
          {#each trayMenuState.recording_devices as device}
            <button
              class="menu-row"
              title={device.name}
              onclick={() => runTrayMenuAction("recording", device.id)}
            >
              <span class="menu-check">{device.is_default ? "✓" : ""}</span>
              <span class="menu-label">{device.name}</span>
            </button>
          {:else}
            <div class="menu-empty">没有可用录音设备</div>
          {/each}
        {:else if activeSubmenu === "style"}
          {#each styleOptions as style}
            <button
              class="menu-row"
              onclick={() => runTrayMenuAction("style", style.id)}
            >
              <span class="menu-check">{trayMenuState.blur_style === style.id ? "✓" : ""}</span>
              <span class="menu-label">{style.label}</span>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>
{:else}
<main>
  {#if hasMergedControls}
  <section class="merged-controls">
    {#if panelVisibility.speaker}
    <div class="control-row">
      <div
        class="icon-box {sysMuted ? 'muted' : ''}"
        title="System Volume"
        onclick={toggleSysMute}
        style="cursor: pointer;"
      >
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
          ><path d="M11 5L6 9H2v6h4l5 4V5z" />
          {#if !sysMuted}
            <path d="M15.54 8.46a5 5 0 0 1 0 7.07" /><path
              d="M19.07 4.93a10 10 0 0 1 0 14.14"
            />
          {:else}
            <line x1="23" y1="9" x2="17" y2="15" /><line
              x1="17"
              y1="9"
              x2="23"
              y2="15"
            />
          {/if}
        </svg>
      </div>
      <div class="slider-container">
        <input
          type="range"
          min="0"
          max="100"
          bind:value={sysVol}
          oninput={setSysVol}
          onpointerdown={handleDragStart}
          onpointerup={handleDragEnd}
        />
        <span class="value-badge">{Math.round(sysVol)}</span>
      </div>
    </div>
    {/if}

    {#if panelVisibility.microphone}
    <div class="control-row">
      <div
        class="icon-box {micMuted ? 'muted' : ''}"
        title="Microphone"
        onclick={toggleMicMute}
        style="cursor: pointer;"
      >
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
          /><line x1="8" y1="22" x2="16" y2="22" />
          {#if micMuted}
            <line x1="1" y1="1" x2="23" y2="23" />
          {/if}
        </svg>
      </div>
      <div class="slider-container">
        <input
          type="range"
          min="0"
          max="100"
          bind:value={micVol}
          oninput={setMicVol}
          onpointerdown={handleDragStart}
          onpointerup={handleDragEnd}
        />
        <span class="value-badge">{Math.round(micVol)}</span>
      </div>
    </div>
    {/if}

    {#if panelVisibility.brightness}
    {#each monitors as monitor (monitor.id)}
    <div
      class="control-row"
      title={brightnessErrors[monitor.id] || `显示器 ${monitor.number} · ${monitor.name}${monitor.brightness === null ? " · 不支持亮度调节，请检查显示器 DDC/CI 设置" : ""}`}
    >
      <div class="icon-box monitor-icon" class:unavailable={monitor.brightness === null}>
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
          aria-hidden="true"
          ><rect x="2" y="3" width="20" height="14" rx="2" /><path
            d="M12 17v4"
          /><path d="M8 21h8" /></svg
        >
        {#if monitors.length > 1}
          <span class="monitor-number" aria-hidden="true">{monitor.number}</span>
        {/if}
      </div>
      <div class="slider-container">
        <input
          type="range"
          min="0"
          max="100"
          aria-label={`显示器 ${monitor.number} · ${monitor.name} 亮度`}
          aria-valuetext={`${Math.round(monitor.brightness_display)}%`}
          disabled={monitor.brightness === null}
          value={monitor.brightness_display}
          oninput={(event) => setBrightness(monitor, event.currentTarget.valueAsNumber)}
          onpointerdown={handleDragStart}
          onpointerup={handleDragEnd}
        />
        <span class="value-badge" class:brightness-error={!!brightnessErrors[monitor.id]}>
          {monitor.brightness === null ? "不可调" : Math.round(monitor.brightness_display)}
        </span>
      </div>
    </div>
    {#if brightnessErrors[monitor.id]}
      <div class="monitor-message brightness-error" role="status">
        显示器 {monitor.number} 调节失败：{brightnessErrors[monitor.id]}
      </div>
    {/if}
    {:else}
      <div class="monitor-message" role="status">
        {monitorLoadError || (monitorsLoaded ? "没有检测到显示器" : "正在检测显示器…")}
      </div>
    {/each}
    {/if}

    {#if panelVisibility.mouse_speed}
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
          oninput={setMouseSpeed}
          onpointerdown={handleDragStart}
          onpointerup={handleDragEnd}
        />
        <span class="value-badge">{mouseSpeed}</span>
      </div>
    </div>
    {/if}
  </section>
  {/if}

  {#if panelVisibility.volume_mixer}
  <section class="app-section">
    <div class="app-list">
      {#each apps as app (app.pid + app.name)}
        <div class="app-row">
          <div
            class="icon-box {app.is_muted ? 'muted' : ''}"
            title={app.name}
            style="cursor: pointer;"
            onclick={() => toggleAppMute(app.pid, app.is_muted)}
          >
            {#if app.icon_path && app.icon_path !== ""}
              <img
                class="app-icon"
                style="filter: {app.is_muted
                  ? 'grayscale(1) opacity(0.5)'
                  : 'none'}"
                src={app.icon_path.startsWith("data:")
                  ? app.icon_path
                  : convertFileSrc(app.icon_path)}
                onerror={(e) => {
                  const target = /** @type {HTMLImageElement} */ (
                    e.currentTarget
                  );
                  target.style.display = "none";
                  // In case of error, the user will see nothing or we could show fallback here,
                  // but to be safe and clean we use Svelte's conditional rendering.
                }}
                alt=""
              />
            {:else}
              <div class="app-icon-fallback">
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
            {/if}
          </div>

          <div class="slider-container">
            <input
              type="range"
              min="0"
              max="100"
              bind:value={app.volume_display}
              oninput={(e) => {
                const v = e.currentTarget.valueAsNumber;
                app.volume = v / 100;
                setAppVol(app.pid, v);
              }}
              onpointerdown={handleDragStart}
              onpointerup={handleDragEnd}
            />
            <span class="value-badge">{app.volume_display}</span>
          </div>
        </div>
      {:else}
        <div class="loading">Scanning sessions...</div>
      {/each}
    </div>
  </section>
  {/if}
</main>
{/if}

<style>
  :global(html),
  :global(body) {
    font-family: "Segoe UI", system-ui, sans-serif;
    background: transparent !important;
    color: #1a1a1a;
    margin: 0;
    padding: 0;
    user-select: none;
    overflow: hidden;
  }

  @media (prefers-color-scheme: dark) {
    :global(body) {
      color: #ffffff;
    }
  }

  main {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px;
    width: 100%;
    height: auto;
    box-sizing: border-box;
    overflow: hidden;
    position: relative;

    /* Base logic: Transparent background, let OS Acrylic show through */
    background: transparent !important;

    /* VISUAL FIX: Enforce rounded corners on the definition of content area */
    border-radius: 12px !important;
    border: 1px solid rgba(255, 255, 255, 0.4);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
  }

  .tray-menu-stage {
    box-sizing: border-box;
    width: 192px;
    height: 286px;
    position: relative;
    padding: 0;
    background: transparent;
    font-family: "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
  }

  .tray-menu-stage.expanded {
    width: 390px;
    height: 340px;
  }

  .tray-menu-stage.playback,
  .tray-menu-stage.recording {
    width: 460px;
  }

  .context-menu,
  .context-submenu {
    width: 176px;
    max-height: 322px;
    box-sizing: border-box;
    padding: 6px;
    border-radius: 8px;
    border: 1px solid rgba(0, 0, 0, 0.07);
    background: rgba(248, 248, 248, 0.9);
    box-shadow:
      0 8px 22px rgba(0, 0, 0, 0.12),
      0 1px 4px rgba(0, 0, 0, 0.1);
    backdrop-filter: blur(26px) saturate(1.35);
    overflow: hidden;
  }

  .context-menu {
    position: absolute;
    left: 8px;
    bottom: 8px;
  }

  .context-submenu {
    position: absolute;
    left: 192px;
    bottom: 8px;
    width: 190px;
    overflow-y: auto;
  }

  .context-submenu.playback,
  .context-submenu.recording {
    width: 260px;
  }

  .context-submenu::-webkit-scrollbar {
    width: 8px;
  }

  .context-submenu::-webkit-scrollbar-thumb {
    background: rgba(0, 0, 0, 0.18);
    border-radius: 999px;
    border: 2px solid transparent;
    background-clip: content-box;
  }

  .menu-row {
    width: 100%;
    height: 40px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: #1f1f1f;
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) 14px;
    align-items: center;
    gap: 5px;
    padding: 0 7px;
    font: inherit;
    font-size: 14px;
    text-align: left;
    cursor: default;
  }

  .menu-row:hover,
  .menu-row.active {
    background: rgba(0, 0, 0, 0.06);
  }

  .menu-icon,
  .menu-check {
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: #202020;
    font-size: 15px;
  }

  .menu-icon svg {
    width: 21px;
    height: 21px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .menu-label {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .menu-chevron {
    justify-self: end;
    color: #484848;
    font-size: 22px;
    line-height: 1;
    transform: translateY(-1px);
  }

  .menu-separator {
    height: 1px;
    margin: 5px -6px;
    background: rgba(0, 0, 0, 0.08);
  }

  .menu-empty {
    padding: 13px 14px;
    color: #6b6b6b;
    font-size: 14px;
  }

  @media (prefers-color-scheme: dark) {
    .context-menu,
    .context-submenu {
      border-color: rgba(255, 255, 255, 0.08);
      background: rgba(38, 38, 38, 0.92);
      box-shadow:
        0 10px 24px rgba(0, 0, 0, 0.32),
        0 1px 4px rgba(0, 0, 0, 0.28);
    }

    .menu-row {
      color: #f2f2f2;
    }

    .menu-row:hover,
    .menu-row.active {
      background: rgba(255, 255, 255, 0.09);
    }

    .menu-icon,
    .menu-check,
    .menu-chevron {
      color: #f2f2f2;
    }

    .menu-separator {
      background: rgba(255, 255, 255, 0.08);
    }

    .menu-empty {
      color: #b8b8b8;
    }

    .context-submenu::-webkit-scrollbar-thumb {
      background: rgba(255, 255, 255, 0.24);
      border: 2px solid transparent;
      background-clip: content-box;
    }
  }

  @media (prefers-color-scheme: dark) {
    main {
      /* background: rgba(28, 28, 28, 0.8); */
      background: transparent !important;
      border: 1px solid rgba(255, 255, 255, 0.08);
      box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
    }
  }

  main::-webkit-scrollbar {
    width: 0px;
  }

  section {
    background: rgba(255, 255, 255, 0.3);
    padding: 8px; /* Tighter padding for alignment */
    border-radius: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px; /* Unified gap for vertical rhythm */
    border: 1px solid rgba(255, 255, 255, 0.2);
    flex-shrink: 0;
  }

  @media (prefers-color-scheme: dark) {
    section {
      background: rgba(255, 255, 255, 0.03);
      border: 1px solid rgba(255, 255, 255, 0.05);
    }
  }

  .control-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px; /* Essential for aligning with app-row */
    border-radius: 4px;
    transition: background-color 0.1s;
  }

  .control-row:hover {
    background: rgba(255, 255, 255, 0.3);
  }

  @media (prefers-color-scheme: dark) {
    /* Global structural styles */
    :global(html),
    :global(body) {
      background-color: transparent !important;
      margin: 0;
      padding: 0;
      overflow: hidden; /* Prevent scrollbars */
      width: 100%;
      height: 100%;
    }

    /* Control Row Hover Effect */
    .control-row:hover {
      background: rgba(255, 255, 255, 0.05);
    }
  }

  .icon-box {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #0067c0; /* Windows 11 default blue */
    transition: transform 0.1s;
  }

  @media (prefers-color-scheme: dark) {
    .icon-box {
      color: #60cdff; /* Lighter blue for dark mode */
    }
  }

  .icon-box.muted {
    color: #999 !important;
  }

  .monitor-icon {
    position: relative;
    flex-shrink: 0;
  }

  .monitor-number {
    position: absolute;
    right: -4px;
    bottom: -3px;
    min-width: 13px;
    height: 13px;
    padding: 0 2px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    background: #0067c0;
    color: #fff;
    font-size: 9px;
    font-weight: 600;
    line-height: 1;
    font-variant-numeric: tabular-nums;
  }

  .monitor-icon.unavailable {
    color: #888;
  }

  .monitor-icon.unavailable .monitor-number {
    background: #888;
  }

  .monitor-message {
    padding: 4px;
    color: #666;
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .brightness-error {
    color: #c42b1c;
  }

  @media (prefers-color-scheme: dark) {
    .monitor-number {
      background: #60cdff;
      color: #202020;
    }

    .monitor-message {
      color: #aaa;
    }

    .brightness-error {
      color: #ff998f;
    }
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
    background: rgba(0, 0, 0, 0.1);
    border-radius: 2px;
    outline: none;
    cursor: pointer;
  }

  @media (prefers-color-scheme: dark) {
    input[type="range"] {
      background: rgba(255, 255, 255, 0.15);
    }
  }

  input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 16px;
    height: 16px;
    background: #0067c0;
    border-radius: 50%;
    border: 3px solid #ffffff;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
    transition: transform 0.1s;
  }

  @media (prefers-color-scheme: dark) {
    input[type="range"]::-webkit-slider-thumb {
      background: #60cdff;
      border-color: #202020;
    }
  }

  input[type="range"]::-webkit-slider-thumb:hover {
    transform: scale(1.1);
  }

  input[type="range"]:disabled {
    cursor: default;
    opacity: 0.4;
  }

  .value-badge {
    min-width: 24px;
    text-align: right;
    font-size: 0.85em;
    color: #666;
    font-feature-settings: "tnum";
    font-weight: 500;
  }

  @media (prefers-color-scheme: dark) {
    .value-badge {
      color: #aaa;
    }
  }

  .app-list {
    display: flex;
    flex-direction: column;
    gap: 4px; /* Same as section gap */
  }

  .app-row {
    padding: 4px; /* Same padding as control-row */
    border-radius: 4px;
    display: flex;
    align-items: center;
    gap: 12px;
    transition: background-color 0.1s;
  }

  .app-row:hover {
    background: rgba(255, 255, 255, 0.3);
  }

  @media (prefers-color-scheme: dark) {
    .app-row:hover {
      background: rgba(255, 255, 255, 0.05);
    }
  }

  .app-icon {
    width: 24px; /* Sync with icon-box width */
    height: 24px;
    object-fit: contain;
    image-rendering: -webkit-optimize-contrast;
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
    gap: 4px; /* Unified with app-list */
  }

  .loading {
    text-align: center;
    padding: 20px;
    color: #999;
    font-size: 0.9em;
    font-style: italic;
  }
</style>
