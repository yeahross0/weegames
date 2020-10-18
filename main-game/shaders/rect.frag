#version 330

out vec4 frag_colour;

uniform vec4 rect_colour;

void main()
{
    frag_colour = rect_colour;
}