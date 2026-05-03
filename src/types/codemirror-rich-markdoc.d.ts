declare module 'codemirror-rich-markdoc' {
  import type { ViewPlugin } from '@codemirror/view';
  import type { Config } from '@markdoc/markdoc';

  export interface MarkdocPluginConfig {
    lezer?: Record<string, unknown>;
    markdoc: Config;
  }

  const richEditor: (config: MarkdocPluginConfig) => ReturnType<typeof ViewPlugin.fromClass>;
  export default richEditor;
}
