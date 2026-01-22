<script>
    import Slider from "./Slider.svelte";

    /** @type {string} */
    export let name;
    /** @type {number} */
    export let pid;
    /** @type {number} */
    export let volume = 1.0;
    /** @type {(pid: number, vol: number) => void} */
    export let onVolumeChange;

    // Volume comes as 0.0-1.0 float, Slider expects 0-100
    // Svelte 4 reactivity
    let sliderVal = Math.round(volume * 100);

    // Watch for volume prop changes from parent
    $: sliderVal = Math.round(volume * 100);

    /** @param {any} e */
    function update(e) {
        onVolumeChange(pid, sliderVal / 100.0);
    }
</script>

<div class="app-row">
    <div class="name" title={name}>{name}</div>
    <Slider
        bind:value={sliderVal}
        max={100}
        on:change={update}
        on:input={update}
    />
</div>

<style>
    .app-row {
        display: flex;
        flex-direction: column;
        padding: 5px 10px;
        background: rgba(255, 255, 255, 0.05);
        margin-bottom: 5px;
        border-radius: 6px;
        flex-shrink: 0; /* Prevent collapsing in restricted height */
        min-height: 50px; /* Ensure content fits */
    }
    .name {
        font-size: 0.85em;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        margin-bottom: 2px;
        color: #eee;
    }
</style>
