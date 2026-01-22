<script>
    export let checked = false;
    export let label = "";
</script>

<label class="toggle-container">
    <span>{label}</span>
    <input type="checkbox" bind:checked on:change />
    <span class="slider"></span>
</label>

<style>
    .toggle-container {
        display: flex;
        justify-content: space-between;
        align-items: center;
        cursor: pointer;
        font-size: 0.9em;
        user-select: none;
        padding: 5px 0;
    }

    input {
        display: none;
    }

    .slider {
        position: relative;
        width: 36px;
        height: 20px;
        background-color: #444;
        border-radius: 20px;
        transition: 0.4s;
        display: inline-block;
    }

    .slider:before {
        position: absolute;
        content: "";
        height: 16px;
        width: 16px;
        left: 2px;
        bottom: 2px;
        background-color: white;
        border-radius: 50%;
        transition: 0.4s;
    }

    input:checked + .slider {
        background-color: #0078d4;
    }

    input:checked + .slider:before {
        transform: translateX(16px);
    }
</style>
