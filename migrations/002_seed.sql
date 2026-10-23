INSERT INTO recipes (id, title, description, servings, prep_minutes, cook_minutes) VALUES
    (1, 'Classic Margherita Pizza',
     'A simple Neapolitan-style pizza with tomato, fresh mozzarella and basil.',
     2, 20, 10);

INSERT INTO ingredients (recipe_id, position, text) VALUES
    (1, 1, '250 g pizza dough, at room temperature'),
    (1, 2, '120 ml crushed San Marzano tomatoes'),
    (1, 3, '125 g fresh mozzarella, torn'),
    (1, 4, 'A handful of fresh basil leaves'),
    (1, 5, '1 tbsp extra virgin olive oil'),
    (1, 6, 'Pinch of sea salt');

INSERT INTO steps (recipe_id, position, text) VALUES
    (1, 1, 'Heat the oven, with a pizza stone or steel inside, to its highest setting for at least 45 minutes.'),
    (1, 2, 'Stretch the dough by hand into a 30 cm round on a floured peel.'),
    (1, 3, 'Spread the tomatoes thinly over the dough, leaving a 2 cm border, and season with salt.'),
    (1, 4, 'Scatter the mozzarella over the top.'),
    (1, 5, 'Slide onto the stone and bake for 8 to 10 minutes, until the crust is blistered.'),
    (1, 6, 'Finish with basil and a drizzle of olive oil. Slice and serve.');
