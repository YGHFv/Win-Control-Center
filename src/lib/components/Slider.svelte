<script>
    export let value = 0;
    export let min = 0;
    export let max = 100;
    export let step = 1;
    export let icon = "";
</script>

<div class="slider-container">
    {#if icon}
        <span class="icon">{icon}</span>
    {/if}
    <!-- Native style slider for maximum compatibility debugging -->
    <input
        type="range"
        {min}
        {max}
        {step}
        bind:value
        class="slider"
        on:input
        on:change
    />
    <span class="value">{Math.round(value)}</span>
</div>

<style>
    .slider-container {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 5px 0;
        width: 100%;
        /* Ensure the container has height */
        min-height: 40px;
    }
    .icon {
        font-size: 1.2em;
        width: 24px;
        text-align: center;
    }
    /* Simple style, rely on browser default appearance */
    .slider {
        flex-grow: 1;
        cursor: pointer;
        height: 30px; /* Large clickable area */
        margin: 0;
        width: 100%;
    }
    .value {
        font-size: 0.9em;
        width: 30px;
        text-align: right;
        color: #ccc;
    }
</style>
