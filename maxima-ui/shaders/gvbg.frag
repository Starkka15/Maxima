precision mediump float;
in vec4 v_color;
in vec2 og_pos;
in vec2 tex_coords;
out vec4 out_color;
uniform sampler2D u_hero;
uniform vec2 u_dimensions;

void main() {
    float gamma = 1.8;
    if (v_color.w == 1.0) {     // main hero image
        float fade0 = smoothstep(0.8, 1, abs(og_pos.x));
        float fade1 = mix(1.0,0.0,fade0);
        fade1 = pow(fade1, gamma/1.0);
        out_color = vec4(pow(texture(u_hero, tex_coords).rgb, vec3(1.0/gamma)) * vec3(fade1), fade1);
    } else {                    // blurred background, space filler
        float Pi = 6.28318530718; // Pi*2
    
        // Gaussian blur, https://www.shadertoy.com/view/Xltfzj
        float Directions = 16.0; // BLUR DIRECTIONS (Default 16.0 - More is better but slower)
        float Quality = 7.0; // BLUR QUALITY (Default 4.0 - More is better but slower)
        float Size = 16.0;
    
        vec2 Radius = Size/u_dimensions;
        
        // Normalized pixel coordinates (from 0 to 1)
        vec2 uv = tex_coords;
        // Pixel colour
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
        out_color = vec4(Color, 1.0);
    }
}