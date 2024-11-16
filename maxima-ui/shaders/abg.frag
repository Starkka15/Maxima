precision mediump float;
in vec3 og_pos;
in vec2 tex_coords;
out vec4 out_color;
uniform sampler2D u_hero;
uniform vec3 u_dimensions;
uniform vec2 u_img_dimensions;
uniform uint u_flags;

const vec3 mistCol = vec3(0.047, 0.106, 0.286);
const vec2 gridSquareSize = vec2(40.0);
const float gridLinesOpacity = 0.2;
const vec2 dotSize = vec2(30.0, 15.0);
const float starSize = 0.025;

float map(float value, float low1, float high1, float low2, float high2) {
    return low2 + (value - low1) * (high2 - low2) / (high1 - low1);
}

float stair(float val, float func) {
    return val * func - floor(val * func);
}

float PHI = 1.61803398874989484820459;  // Φ = Golden Ratio

float gold_noise(vec2 xy, in float seed){
       return fract(tan(distance(xy*PHI, xy)*seed)*xy.x);
}

float grid(vec2 fragCoord) {
    if (mod(fragCoord.x, gridSquareSize.x) <= 2.0) {
        return gridLinesOpacity;
    }
    else if (mod(fragCoord.y, gridSquareSize.y) <= 2.0) {
        return gridLinesOpacity;
    }
    return 0.0;
}

float stars(vec2 res, vec2 uv, vec2 pixel, vec2 cellSize) {
    vec2 uvFlipped = vec2(uv.x, 1.0 - uv.y);
    vec2 cellCoord = vec2(floor(uv.x * res.x / cellSize.x), floor(uvFlipped.y * res.y / cellSize.y));



    float seedCellX = gold_noise(vec2(cellCoord.x+1.0,cellCoord.y+1.0), 420.69);
    float seedCellY = gold_noise(vec2(cellCoord.y+1.0,cellCoord.x+1.0), 42.069);

    seedCellX *= (0.5 - starSize);
    seedCellY *= (0.5 - starSize);

    float x = stair(uvFlipped.x, res.x / cellSize.x);
    float y = stair(uvFlipped.y, res.y / cellSize.y);
    float dist = distance(vec2(x,y), vec2(0.5) + vec2(seedCellX,seedCellY));
    if (dist > starSize) dist = 1.0;

    dist = map(dist, 0.0, starSize, 0.0, 1.0);
    //show cell bounds
    //if (x > 0.99 || x < 0.01) dist = 0.0;
    //if (y > 0.99 || y < 0.01) dist = 0.0;
    dist = 1.0 - dist;
    if (dist < 0.0) dist = 0.0;
    if (dist > 1.0) dist = 1.0;
    return dist;
}

vec3 pattern() {
    vec2 uv = tex_coords.xy;
    vec2 uvFlipped = tex_coords.xy;
    vec2 fragCoordFlipped = vec2(gl_FragCoord.x, u_dimensions.y - gl_FragCoord.y);

    float dist = distance(tex_coords, vec2(1.0));
    dist = map(dist, 0.125, 0.65, 0.0, 1.0);
    if (dist > 0.5) dist = map(dist, 0.5, 1.0, 0.5, 0.0);
    dist *= 2.0;
    dist = map(dist, 0.0, 1.0, 0.0, 0.7);

    vec3 col = vec3(dist) * mistCol;

    uvFlipped.x = stair(uv.x, u_dimensions.x / dotSize.x);
    uvFlipped.y = stair(uv.y, u_dimensions.y / dotSize.y);

    float dotGrid = map(distance(uvFlipped, vec2(0.5)), 0.0, 0.5, 0.0, 1.0);
    col += vec3(1.0 - dotGrid) * dist * 0.471 * mistCol;
    col += grid(fragCoordFlipped) * mistCol;

    col += vec3(stars(u_dimensions.xy, uv, gl_FragCoord.xy, vec2(300.0))) * mistCol;
    col += vec3(stars(u_dimensions.xy, uv, gl_FragCoord.xy, vec2(150.0))) * mistCol * 0.5;

    return col;
}

vec3 bg() {
    if ((int(u_flags) & 0x000001) > 0) {
        return texture(u_hero, tex_coords).rgb * 0.25;
    }

    float gamma = 1.8;
    float Pi = 6.28318530718; // Pi*2

    // Gaussian blur, https://www.shadertoy.com/view/Xltfzj
    float Directions = 16.0; // BLUR DIRECTIONS (Default 16.0 - More is better but slower)
    float Quality = 7.0; // BLUR QUALITY (Default 4.0 - More is better but slower)
    float Size = 10.0;

    vec2 Radius = Size/u_img_dimensions.xy;
    
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
    return Color;
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
    out_color = vec4(1.0);
    if (og_pos.z == 0.0) {
        if (u_dimensions.z == 1.0) {
            out_color.xyz = bg();
        } else if (u_dimensions.z == 0.0) {
            out_color.xyz = pattern();
        } else {
            out_color.xyz = mix(pattern(), bg(), u_dimensions.z);
        }
    } else {
        vignette();
        
    }
}