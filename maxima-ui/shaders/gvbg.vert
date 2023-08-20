const vec2 verts[12] = vec2[12](
    vec2(-1.0, -1.0),
    vec2( 1.0, -1.0),
    vec2( 1.0,  1.0),
    vec2(-1.0, -1.0),
    vec2( 1.0,  1.0),
    vec2(-1.0,  1.0),

    vec2(-1.0, -1.0),
    vec2( 1.0, -1.0),
    vec2( 1.0,  1.0),
    vec2(-1.0, -1.0),
    vec2( 1.0,  1.0),
    vec2(-1.0,  1.0)
);
const vec4 colors[12] = vec4[12](
    vec4(0.0, 0.0, 0.0, 1.0),
    vec4(1.0, 0.0, 0.0, 1.0),
    vec4(1.0, 1.0, 0.0, 1.0),
    vec4(0.0, 0.0, 0.0, 1.0),
    vec4(1.0, 1.0, 0.0, 1.0),
    vec4(0.0, 1.0, 0.0, 1.0),

    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(0.0, 0.0, 0.0, 0.0)
);
//the extra miniscule vram hit is likely better than doing a bunch of weird flipping and scaling every frame
const vec2 uvs[12] = vec2[12](
    vec2(0.0,1.0),
    vec2(1.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,0.0),
    vec2(0.0,1.0),
    vec2(1.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,0.0)
);
out vec4 v_color;
out vec2 og_pos;
out vec2 tex_coords;
uniform vec2 u_dimensions;
void main() {
    v_color = colors[gl_VertexID];
    vec2 vpos = verts[gl_VertexID];
    og_pos = verts[gl_VertexID];
    tex_coords = uvs[gl_VertexID];

    //i worked this out in desmos btw
    if(gl_VertexID < 6){
        vpos.x *= ((u_dimensions.y/9) * 16) / u_dimensions.x;
        // MAYBE do extra logic to shrink the main hero image to make it wider,
        // gradients become (visually, not technically) easier
    }
    else
        vpos.y *= ((u_dimensions.x/16) * 9) / u_dimensions.y;
    gl_Position = vec4(vpos, 0.0, 1.0);
    
}