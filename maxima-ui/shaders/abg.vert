precision mediump float;
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
//the extra miniscule vram hit is likely better than doing a bunch of weird flipping and scaling every frame
const vec2 uvs[12] = vec2[12](
    vec2(0.0,1.0),
    vec2(1.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,0.0),
    // could probably make this loop, but anyone playing games has 192 bytes of vram to spare
    vec2(0.0,1.0),
    vec2(1.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,1.0),
    vec2(1.0,0.0),
    vec2(0.0,0.0)
);
out vec3 og_pos;
out vec2 tex_coords;
uniform vec2 u_dimensions;
uniform vec2 u_img_dimensions;
void main() {
    vec2 vpos = vec2(verts[gl_VertexID].x, verts[gl_VertexID].y);
    og_pos = vec3(vpos.x,vpos.y, gl_VertexID > 5?1.0:0.0);
    tex_coords = uvs[gl_VertexID];
    float src_aspect = u_dimensions.x / u_dimensions.y;
    float dst_aspect = u_img_dimensions.x / u_img_dimensions.y;
    
    bool layer = (gl_VertexID > 5);
    if (!layer) {
        if ( (src_aspect / dst_aspect) < 1.0 ) {        // Taller
            vpos.x *= ((u_dimensions.y/u_img_dimensions.y) * u_img_dimensions.x) / u_dimensions.x;
        } else if ( (src_aspect / dst_aspect) > 1.0 ) { // Wider
            vpos.y *= ((u_dimensions.x/u_img_dimensions.x) * u_img_dimensions.y) / u_dimensions.y;
        }
    }

    gl_Position = vec4(vpos, og_pos.z, 1.0);
    
}