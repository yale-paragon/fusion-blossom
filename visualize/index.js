import * as THREE from 'three'
import { OrbitControls } from 'OrbitControls'
import * as Stats from 'Stats'
import * as GUI from './gui3d.js'
const { ref, reactive } = Vue

// fetch fusion blossom runtime data
const urlParams = new URLSearchParams(window.location.search)
const filename = urlParams.get('filename') || "default.json"

var fusion_data

// create vue3 app
const App = {
    setup() {
        return {
            error_message: ref(null),
            snapshot_num: ref(1),
            snapshot_select: ref(0),
            snapshot_select_label: ref(""),
            snapshot_labels: reactive([]),
            use_perspective_camera: GUI.use_perspective_camera,
            scale: GUI.scale,
            size: GUI.default_size,
            // GUI related states
            show_stats: ref(false),
        }
    },
    async mounted() {
        console.log(this.size)
        try {
            let response = await fetch('./data/' + filename, { cache: 'no-cache', })
            fusion_data = await response.json()
            console.log(fusion_data)
        } catch (e) {
            this.error_message = "fetch file error"
            throw e
        }
        this.show_snapshot(fusion_data.snapshots[0][1])  // load the first snapshot
        this.snapshot_num = fusion_data.snapshots.length
        for (let [idx, [name, _]] of fusion_data.snapshots.entries()) {
            this.snapshot_labels.push(`[${idx}] ${name}`)
        }
        this.snapshot_select_label = this.snapshot_labels[0]
        // only if data loads successfully will the animation starts
        GUI.animate()
        // add keyboard shortcuts
        document.onkeydown = (event) => {
            if (!event.metaKey) {
                if (event.key == "t" || event.key == "T") {
                    this.reset_camera("top")
                } else if (event.key == "l" || event.key == "L") {
                    this.reset_camera("left")
                } else if (event.key == "f" || event.key == "F") {
                    this.reset_camera("front")
                } else if (event.key == "ArrowRight") {
                    if (this.snapshot_select < this.snapshot_num - 1) {
                        this.snapshot_select += 1
                    }
                } else if (event.key == "ArrowLeft") {
                    if (this.snapshot_select > 0) {
                        this.snapshot_select -= 1
                    }
                } else {
                    return  // unrecognized, propagate to other listeners
                }
                event.preventDefault()
                event.stopPropagation()
            }
        }
    },
    methods: {
        show_snapshot(snapshot) {
            try {
                GUI.show_snapshot(snapshot, fusion_data)
            } catch (e) {
                this.error_message = "load data error"
                throw e
            }
        },
        reset_camera(direction) {
            GUI.reset_camera_position(direction)
        },
    },
    watch: {
        snapshot_select() {
            // console.log(this.snapshot_select)
            this.show_snapshot(fusion_data.snapshots[this.snapshot_select][1])  // load the snapshot
            this.snapshot_select_label = this.snapshot_labels[this.snapshot_select]
        },
        snapshot_select_label() {
            this.snapshot_select = parseInt(this.snapshot_select_label.split(']')[0].split('[')[1])
        },
    },
    computed: {
        vertical_thumb_style() {
            return {
                right: `${4*this.scale}px`,
                borderRadius: `${5*this.scale}px`,
                backgroundColor: '#027be3',
                width: `${5*this.scale}px`,
                opacity: 0.75
            }
        },
        horizontal_thumb_style() {
            return {
                bottom: `${4*this.scale}px`,
                borderRadius: `${5*this.scale}px`,
                backgroundColor: '#027be3',
                height: `${5*this.scale}px`,
                opacity: 0.75
            }
        },
        vertical_bar_style() {
            return {
                right: `${2*this.scale}px`,
                borderRadius: `${9*this.scale}px`,
                backgroundColor: '#027be3',
                width: `${9*this.scale}px`,
                opacity: 0.2
            }
        },
        horizontal_bar_style() {
            return {
                bottom: `${2*this.scale}px`,
                borderRadius: `${9*this.scale}px`,
                backgroundColor: '#027be3',
                height: `${9*this.scale}px`,
                opacity: 0.2
            }
        },
    },
}
const app = Vue.createApp(App)
app.use(Quasar)
Quasar.Screen.setSizes({ sm: 1200, md: 1600, lg: 2880, xl: 3840 })
app.mount("#app")
window.app = app
