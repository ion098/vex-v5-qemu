<script lang="ts">
    import {
        type NodeProps,
        type Node,
        Position,
        useSvelteFlow,
    } from "@xyflow/svelte";
    import { drag } from "~/lib/actions";
    import { SmartPortHandle } from "~/lib/handles";
    import { GPSSensor } from "~/lib/icons";
    import { NodeBase } from "~/lib/components";

    const { screenToFlowPosition } = useSvelteFlow();

    type NodeData = {
        capacity: number;
        temperature: number;
        current: number;
        voltage: number;
    };

    type $$Props = NodeProps<Node<NodeData>>;

    export let data: NodeData;
    export let id: $$Props["id"];

    let position = { x: 150 / 2 - 11, y: 150 / 2 - 11 };
    let draggableContainer: HTMLDivElement;

    function moveDraggable(e: MouseEvent) {
        let flowCoords = screenToFlowPosition({ x: e.clientX, y: e.clientY });
        let boundingRect = draggableContainer.getBoundingClientRect();
        let boundingCoords = screenToFlowPosition({
            x: boundingRect.left,
            y: boundingRect.top,
        });
        let relativeX = flowCoords.x - boundingCoords.x - 14;
        let relativeY = flowCoords.y - boundingCoords.y - 14;
        position = { x: relativeX, y: relativeY };
        position = {
            x: Math.max(0, Math.min(150 - 28, position.x)),
            y: Math.max(0, Math.min(150 - 28, position.y)),
        };
    }

    data;
</script>

<NodeBase title="GPS Sensor">
    <SmartPortHandle
        slot="handle"
        id="connector"
        type="source"
        parentNode={id}
        position={Position.Left}
    />
    <GPSSensor slot="icon" size="16" />

    <div class="coordinates">
        <span>x: {Math.round(position.x * 10) / 10}</span>
        <span>x: {Math.round(position.y * 10) / 10}</span>
    </div>
    <div bind:this={draggableContainer} class="position nodrag">
        <div
            class="draggable"
            style="left: {position.x}px; top: {position.y}px"
            use:drag={(event) => {
                if (!draggableContainer) return;
                moveDraggable(event);
            }}
        >
            <svg width="24px" height="24"
                ><circle
                    cx="12"
                    cy="12"
                    r="11"
                    stroke="currentColor"
                    fill="none"
                    stroke-width="2"
                /></svg
            >
        </div>
    </div>
</NodeBase>

<style>
    .coordinates {
        display: flex;
        gap: 10px;
        width: 100%;
    }

    .position {
        background-color: var(--background-secondary);
        width: 150px;
        height: 150px;
        border-radius: 16px;
        border: var(--accent-primary) 2px solid;
        cursor: default;
    }

    .draggable {
        position: relative;
        cursor: grab;
    }

    .draggable:hover {
        color: var(--foreground-primary);
    }

    .draggable:active {
        cursor: grabbing;
    }
</style>
