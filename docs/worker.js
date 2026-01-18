/**
 * Cloudflare Worker for serving Catapult documentation
 *
 * This worker serves static files from the assets directory
 * with proper caching headers and SPA-style routing.
 */

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    let path = url.pathname;

    // Serve index.html for root
    if (path === '/' || path === '') {
      path = '/index.html';
    }

    // Try to serve the file from assets
    try {
      // env.ASSETS is automatically available when using [assets] in wrangler.toml
      const response = await env.ASSETS.fetch(request);

      if (response.status === 404) {
        // For SPA-style routing, try serving index.html for HTML requests
        if (!path.includes('.') || path.endsWith('.html')) {
          const indexRequest = new Request(new URL('/index.html', request.url), request);
          const indexResponse = await env.ASSETS.fetch(indexRequest);
          if (indexResponse.status === 200) {
            return indexResponse;
          }
        }

        // Serve 404 page
        const notFoundRequest = new Request(new URL('/404.html', request.url), request);
        const notFoundResponse = await env.ASSETS.fetch(notFoundRequest);
        if (notFoundResponse.status === 200) {
          return new Response(notFoundResponse.body, {
            status: 404,
            headers: notFoundResponse.headers
          });
        }

        return new Response('Not Found', { status: 404 });
      }

      // Add caching headers for static assets
      const headers = new Headers(response.headers);

      if (path.match(/\.(js|css|woff2?|ttf|eot)$/)) {
        // Long cache for immutable assets
        headers.set('Cache-Control', 'public, max-age=31536000, immutable');
      } else if (path.match(/\.(png|jpg|jpeg|gif|svg|ico)$/)) {
        // Medium cache for images
        headers.set('Cache-Control', 'public, max-age=86400');
      } else {
        // Short cache for HTML
        headers.set('Cache-Control', 'public, max-age=3600');
      }

      return new Response(response.body, {
        status: response.status,
        headers
      });
    } catch (e) {
      return new Response('Internal Error', { status: 500 });
    }
  }
};
