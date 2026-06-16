import { BronzeDrum3D } from './bronze_drum_3d.js';
import { AcousticPanel } from './acoustic_panel.js';

const drum3D = new BronzeDrum3D('three-canvas');
const panel = new AcousticPanel(drum3D);

panel.init();
