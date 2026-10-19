/**
 * High-performance WebGL2 Hardware-Accelerated Canvas Renderer for IronRDP.
 *
 * Implements GPU-accelerated dirty-rectangle texture updates (gl.texSubImage2D)
 * and hardware bilinear/bicubic display scaling for remote desktop rendering.
 */

export class RdpWebGlRenderer {
  private canvas: HTMLCanvasElement | null = null;
  private gl: WebGL2RenderingContext | null = null;
  private program: WebGLProgram | null = null;
  private vao: WebGLVertexArrayObject | null = null;
  private vbo: WebGLBuffer | null = null;
  private texture: WebGLTexture | null = null;

  private textureWidth = 0;
  private textureHeight = 0;
  private isContextLost = false;

  private onContextLostBound = (e: Event) => this.handleContextLost(e);
  private onContextRestoredBound = () => this.handleContextRestored();

  /**
   * Initializes the WebGL2 context, shaders, and geometry on the canvas.
   * Returns true on success, or false if WebGL2 is not supported.
   */
  public init(canvas: HTMLCanvasElement): boolean {
    this.canvas = canvas;

    const gl = canvas.getContext('webgl2', {
      alpha: false,
      depth: false,
      stencil: false,
      antialias: false,
      preserveDrawingBuffer: true,
      powerPreference: 'high-performance',
    });

    if (!gl) {
      return false;
    }

    this.gl = gl;
    this.canvas.addEventListener('webglcontextlost', this.onContextLostBound);
    this.canvas.addEventListener('webglcontextrestored', this.onContextRestoredBound);

    return this.setupPipeline();
  }

  private setupPipeline(): boolean {
    const gl = this.gl;
    if (!gl) return false;

    // Vertex shader: full-screen quad mapped to RDP top-left (0,0) -> bottom-right (1,1) UVs
    const vsSource = `#version 300 es
      in vec2 a_position;
      in vec2 a_texCoord;
      out vec2 v_texCoord;
      void main() {
        gl_Position = vec4(a_position, 0.0, 1.0);
        v_texCoord = a_texCoord;
      }
    `;

    // Fragment shader: texture lookup
    const fsSource = `#version 300 es
      precision highp float;
      uniform sampler2D u_desktopTexture;
      in vec2 v_texCoord;
      out vec4 fragColor;
      void main() {
        fragColor = texture(u_desktopTexture, v_texCoord);
      }
    `;

    const vs = this.compileShader(gl.VERTEX_SHADER, vsSource);
    const fs = this.compileShader(gl.FRAGMENT_SHADER, fsSource);
    if (!vs || !fs) return false;

    const program = gl.createProgram();
    if (!program) return false;

    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.error('WebGL2 program link failed:', gl.getProgramInfoLog(program));
      gl.deleteProgram(program);
      return false;
    }

    gl.deleteShader(vs);
    gl.deleteShader(fs);
    this.program = program;

    // Full screen triangle strip quad
    // Quad vertices: position [x, y], texCoord [u, v]
    // Top-Left (-1, 1) -> (0, 0), Bottom-Right (1, -1) -> (1, 1)
    // Matches RDP coordinate orientation without CPU Y-flips
    const quadData = new Float32Array([
      -1.0,  1.0, 0.0, 0.0, // Top-Left
       1.0,  1.0, 1.0, 0.0, // Top-Right
      -1.0, -1.0, 0.0, 1.0, // Bottom-Left
       1.0, -1.0, 1.0, 1.0, // Bottom-Right
    ]);

    const vao = gl.createVertexArray();
    const vbo = gl.createBuffer();
    if (!vao || !vbo) return false;

    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, quadData, gl.STATIC_DRAW);

    const aPosLoc = gl.getAttribLocation(program, 'a_position');
    const aTexLoc = gl.getAttribLocation(program, 'a_texCoord');

    const stride = 4 * Float32Array.BYTES_PER_ELEMENT;
    gl.enableVertexAttribArray(aPosLoc);
    gl.vertexAttribPointer(aPosLoc, 2, gl.FLOAT, false, stride, 0);

    gl.enableVertexAttribArray(aTexLoc);
    gl.vertexAttribPointer(aTexLoc, 2, gl.FLOAT, false, stride, 2 * Float32Array.BYTES_PER_ELEMENT);

    gl.bindVertexArray(null);

    this.vao = vao;
    this.vbo = vbo;

    // Setup GPU desktop texture
    const texture = gl.createTexture();
    if (!texture) return false;

    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);

    this.texture = texture;
    this.textureWidth = 0;
    this.textureHeight = 0;

    return true;
  }

  private compileShader(type: number, source: string): WebGLShader | null {
    const gl = this.gl;
    if (!gl) return null;

    const shader = gl.createShader(type);
    if (!shader) return null;

    gl.shaderSource(shader, source);
    gl.compileShader(shader);

    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      console.error('WebGL2 shader compile error:', gl.getShaderInfoLog(shader));
      gl.deleteShader(shader);
      return null;
    }

    return shader;
  }

  /**
   * Resizes the GPU texture if desktop dimensions change.
   */
  public ensureSize(desktopWidth: number, desktopHeight: number): void {
    const gl = this.gl;
    if (!gl || this.isContextLost || !this.texture) return;

    if (this.textureWidth !== desktopWidth || this.textureHeight !== desktopHeight) {
      this.textureWidth = desktopWidth;
      this.textureHeight = desktopHeight;

      gl.bindTexture(gl.TEXTURE_2D, this.texture);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        desktopWidth,
        desktopHeight,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        null
      );
    }
  }

  /**
   * Fast hardware texture sub-image update for a dirty rectangle tile.
   */
  public uploadTile(
    x: number,
    y: number,
    width: number,
    height: number,
    rgbaPixels: Uint8Array
  ): void {
    const gl = this.gl;
    if (!gl || this.isContextLost || !this.texture) return;

    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texSubImage2D(
      gl.TEXTURE_2D,
      0,
      x,
      y,
      width,
      height,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      rgbaPixels
    );
  }

  /**
   * Executes a GPU render pass presenting the composite desktop to the canvas.
   */
  public render(): void {
    const gl = this.gl;
    if (!gl || this.isContextLost || !this.program || !this.vao || !this.texture) return;

    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
    gl.useProgram(this.program);
    gl.bindVertexArray(this.vao);

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);

    const texUniform = gl.getUniformLocation(this.program, 'u_desktopTexture');
    gl.uniform1i(texUniform, 0);

    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    gl.bindVertexArray(null);
  }

  private handleContextLost(e: Event): void {
    e.preventDefault();
    console.warn('WebGL2 context lost in IronRDP viewport');
    this.isContextLost = true;
  }

  private handleContextRestored(): void {
    console.info('WebGL2 context restored; recreating pipeline');
    this.isContextLost = false;
    this.setupPipeline();
  }

  /**
   * Cleans up all GPU memory and event listeners.
   */
  public destroy(): void {
    if (this.canvas) {
      this.canvas.removeEventListener('webglcontextlost', this.onContextLostBound);
      this.canvas.removeEventListener('webglcontextrestored', this.onContextRestoredBound);
      this.canvas = null;
    }

    const gl = this.gl;
    if (gl) {
      if (this.texture) gl.deleteTexture(this.texture);
      if (this.vbo) gl.deleteBuffer(this.vbo);
      if (this.vao) gl.deleteVertexArray(this.vao);
      if (this.program) gl.deleteProgram(this.program);
      this.gl = null;
    }

    this.program = null;
    this.vao = null;
    this.vbo = null;
    this.texture = null;
    this.textureWidth = 0;
    this.textureHeight = 0;
  }
}
