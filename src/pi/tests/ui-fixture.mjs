// Behavioral fixtures for the shipping tool renderers.
import {toolRenderers} from '../extensions/ui/index.ts';
export const editRenderers = () => toolRenderers().get('edit');
export const writeRenderers = () => toolRenderers().get('write');
export const compactRenderers = name => toolRenderers().get(name);
