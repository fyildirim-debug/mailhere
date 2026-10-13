import '@fontsource-variable/geist/wght.css';
import './styles/tokens.css';
import './styles/global.css';

import { mount } from 'svelte';
import App from './App.svelte';
import { initSettings } from './lib/stores/settings.svelte';

void initSettings();

mount(App, { target: document.getElementById('app')! });
