
uniform vec2 viewSize;

attribute vec2 vertex;
attribute vec2 tcoord;

varying vec2 ftcoord;
varying vec2 fpos;

void main(void) {
    ftcoord = tcoord;
    fpos = vertex;

    gl_Position = vec4(2.0 * vertex.x / viewSize.x - 1.0, 1.0 - 2.0 * vertex.y / viewSize.y, 0, 1);
}
