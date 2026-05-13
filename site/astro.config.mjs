import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://plx.github.io',
  base: '/ferric-rules',
  integrations: [
    starlight({
      title: 'ferric-rules',
      description:
        'A mostly CLIPS-compatible forward-chaining rules engine in Rust, designed for embedding as independent engine instances.',
      logo: {
        src: './src/assets/ferric-rules-mark-v1.svg',
        alt: 'ferric-rules',
      },
      favicon: '/favicon.svg',
      customCss: ['./src/styles/starlight.css'],
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/prb/ferric-rules',
        },
      ],
      editLink: {
        baseUrl: 'https://github.com/prb/ferric-rules/edit/main/site/',
      },
      sidebar: [
        {
          label: 'Start Here',
          items: [
            { slug: 'docs', label: 'Overview' },
            { slug: 'docs/getting-started', label: 'Getting started' },
            { slug: 'docs/compatibility', label: 'CLIPS compatibility' },
          ],
        },
        {
          label: 'Embedding',
          items: [
            { slug: 'docs/embedding', label: 'Embedding API' },
            { slug: 'docs/performance', label: 'Performance' },
            { slug: 'docs/internals', label: 'Internals' },
          ],
        },
      ],
    }),
  ],
});
