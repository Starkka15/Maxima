precision mediump float;
in vec3 og_pos;
in vec2 tex_coords;
out vec4 out_color;
uniform sampler2D u_hero;
uniform vec2 u_dimensions;



void bg() {
    float gamma = 1.8;
    float Pi = 6.28318530718; // Pi*2

    // Gaussian blur, https://www.shadertoy.com/view/Xltfzj
    float Directions = 16.0; // BLUR DIRECTIONS (Default 16.0 - More is better but slower)
    float Quality = 13.0; // BLUR QUALITY (Default 4.0 - More is better but slower)
    float Size = 32.0;

    vec2 Radius = Size/u_dimensions;
    
    // Normalized pixel coordinates (from 0 to 1)
    vec2 uv = tex_coords;
    // Pixel color
    vec3 Color = texture(u_hero, tex_coords).rgb;
    
    // Blur calculations
    for( float d=0.0; d<Pi; d+=Pi/Directions)
    {
        for(float i=1.0/Quality; i<=1.0; i+=1.0/Quality)
        {
            Color += texture(u_hero, uv+vec2(cos(d),sin(d))*Radius*i).rgb;
        }
    }

    // Output to screen
    Color /= Quality * Directions - 15.0;
    Color = pow(Color, vec3(1.0/gamma));
    Color *= 0.25;
    out_color = vec4(Color, 1.0);
}

void vignette() {
    vec2 uvv = og_pos.xy + vec2(1.0);
    //uvv *= 2.0;

    if (uvv.x > 1.0) uvv.x = abs(2.0 - uvv.x);
    if (uvv.y > 1.0) uvv.y = abs(2.0 - uvv.y);

    float vig = uvv.x*uvv.y*15.0;
    vig = pow(vig, 0.5);
    vig = min(vig, 1.0);
    vig = max(vig, 0.2);

    out_color = vec4(0.0,0.0,0.0, 1.0-vig);
}

void main() {
    if (og_pos.z == 0.0) {
        bg();
    } else {
        vignette();
    }
}