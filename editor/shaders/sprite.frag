#version 330 core

in vec2 tex_coords;
out vec4 colour;

uniform sampler2D image;
uniform vec4 sprite_colour;

void main()
{    
    colour = sprite_colour * texture(image, tex_coords);
} 