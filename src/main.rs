extern crate sfml;

use std::fs;

use sfml::audio::Music;
use sfml::graphics::{CircleShape, Color, Font, RenderTarget, RenderWindow, Shape, Sprite, Text, Texture, Transformable};
use sfml::system::{self, Clock, Vector2, Vector2f};
use sfml::window::{ContextSettings, Event, Key, Style};

use rand::Rng;
use sfml::SfBox;

const WINDOW_SIZE_X: u32 = 800;
const WINDOW_SIZE_Y: u32 = 600;
const PLAYER_RADIUS: f32 = 20.0;
const INITIAL_BALL_COUNT: u32 = 5;
const PLAYER_SPEED: f32 = 300.0;
const HEALTH_BOX_SPAWN_INTERVAL: f32 = 20.0;
const HEALTH_BOX_HEAL_AMOUNT: i32 = 5;
const HEALTH_BOX_SIZE: f32 = 32.0;
const INITIAL_HP: i32 = 10;
const HIGHSCORE_FILE: &str = "high_score.txt";
const HP_REGEN_INTERVAL: f32 = 5.0;

struct Ball<'a> {
    shape: CircleShape<'a>,
    velocity: Vector2f,
}

impl<'a> Ball<'a> {
    fn new(position: Vector2f, velocity: Vector2f, radius: f32, color: Color) -> Self {
        let mut shape = CircleShape::new(radius, 30);
        shape.set_fill_color(color);
        shape.set_position(position);
        Ball { shape, velocity }
    }

    fn update(&mut self, delta_time: f32) {
        let mut position = self.shape.position();
        position += self.velocity * delta_time;

        if position.x < 0.0 || position.x > WINDOW_SIZE_X as f32 - self.shape.radius() * 2.0 {
            self.velocity.x = -self.velocity.x;
        }
        if position.y < 0.0 || position.y > WINDOW_SIZE_Y as f32 - self.shape.radius() * 2.0 {
            self.velocity.y = -self.velocity.y;
        }

        self.shape.set_position(position);
    }
}

struct Player<'a> {
    shape: CircleShape<'a>,
    hp: i32,
    survival_time: f32,
    time_since_last_hit: f32,
    hit_sound: Music<'a>,
    health_sound: Music<'a>,
    bonus_sound: Music<'a>,
    colors: Vec<Color>,
    current_color_index: usize,
    hits: i32,
    bonuses: i32
}

impl<'a> Player<'a> {
    fn new(position: Vector2f) -> Self {
        let mut shape = CircleShape::new(PLAYER_RADIUS, 30);
        shape.set_position(position);
        
        let colors = vec![
            Color::GREEN,
            Color::BLUE,
            Color::RED,
            Color::YELLOW,
            Color::MAGENTA,
            Color::CYAN,
            Color::WHITE
        ];

        Player
        {
            shape,
            hp: INITIAL_HP,
            survival_time: 0.0,
            time_since_last_hit: 0.0,
            hit_sound: Music::from_file("hit.ogg").expect("Failed to load hit sound effect"),
            health_sound: Music::from_file("health.ogg").expect("Failed to load health sound effect"),
            bonus_sound: Music::from_file("bonus.ogg").expect("Failed to load bonus sound effect"),
            colors,
            current_color_index: 0,
            hits: 0,
            bonuses: 0
        }
    }

    fn select_next_color(&mut self) {
        self.current_color_index = (self.current_color_index + 1) % self.colors.len();
        self.shape.set_fill_color(self.colors[self.current_color_index]);
    }
    
    fn select_previous_color(&mut self) {
        if self.current_color_index == 0 {
            self.current_color_index = self.colors.len() - 1;
        } else {
            self.current_color_index -= 1;
        }
        self.shape.set_fill_color(self.colors[self.current_color_index]);
    }

    fn update(&mut self, delta_time: f32) {
        let mut position = self.shape.position();

        if (Key::is_pressed(Key::W) || Key::is_pressed(Key::Up)) && position.y > 0.0 {
            position.y -= PLAYER_SPEED * delta_time;
        }
        if (Key::is_pressed(Key::S) || Key::is_pressed(Key::Down)) && position.y < WINDOW_SIZE_Y as f32 - PLAYER_RADIUS * 2.0 {
            position.y += PLAYER_SPEED * delta_time;
        }
        if (Key::is_pressed(Key::A) || Key::is_pressed(Key::Left)) && position.x > 0.0 {
            position.x -= PLAYER_SPEED * delta_time;
        }
        if (Key::is_pressed(Key::D) || Key::is_pressed(Key::Right)) && position.x < WINDOW_SIZE_X as f32 - PLAYER_RADIUS * 2.0 {
            position.x += PLAYER_SPEED * delta_time;
        }

        self.shape.set_position(position);
        self.survival_time += delta_time;
        self.time_since_last_hit -= delta_time;

        if self.time_since_last_hit <= 0.0 {
            play_sound(&mut self.bonus_sound);
            self.hp += 1;
            self.bonuses += 1;
            self.time_since_last_hit = HP_REGEN_INTERVAL;
        }
    }

    fn check_collision_with_ball(&mut self, ball: &Ball) -> bool {
        if self.get_distance_to(ball.shape.position()) < PLAYER_RADIUS + ball.shape.radius() {
            play_sound(&mut self.hit_sound);
            self.hp -= 1;
            self.time_since_last_hit = HP_REGEN_INTERVAL;
            self.hits += 1;
            return true;
        }
        false
    }

    fn check_collision_with_health_box(&mut self, health_box: &HealthBox) -> bool {
        if self.get_distance_to(health_box.sprite.position()) < PLAYER_RADIUS + HEALTH_BOX_SIZE / 2.0 {
            play_sound(&mut self.health_sound);
            self.hp += HEALTH_BOX_HEAL_AMOUNT;
            return true;
        }
        false
    }

    fn get_distance_to(&mut self, position: Vector2<f32>) -> f32 {
        let player_pos = self.shape.position();
        ((player_pos.x - position.x).powi(2) + (player_pos.y - position.y).powi(2)).sqrt()
    }
}

struct HealthBox<'a> {
    sprite: Sprite<'a>,
}

impl<'a> HealthBox<'a> {
    fn new(texture: &'a Texture, position: Vector2f) -> Self {
        let mut sprite = Sprite::with_texture(texture);
        sprite.set_position(position);
        HealthBox { sprite }
    }
}

struct Game<'a> {
    state: GameState,
    high_score: f32,
    clock: SfBox<Clock>,
    music_menu: Music<'a>,
    music_game: Music<'a>,
    sound_game_over: Music<'a>,
    player: Player<'a>,
}

enum GameState {
    Menu,
    Playing,
    GameOver,
}

impl<'a> Game<'a> {
    fn new() -> Self {
        let mut game = Game {
            state: GameState::Menu,
            high_score: 0.0,
            clock: Clock::start(),
            music_menu: Music::from_file("menu.ogg").expect("Failed to load menu music"),
            music_game: Music::from_file("game.ogg").expect("Failed to load game music"),
            sound_game_over: Music::from_file("gameOver.ogg").expect("Failed to load game over sound"),
            player: Player::new(Vector2f::new(WINDOW_SIZE_X as f32 / 2.0, WINDOW_SIZE_Y as f32 / 2.0)),
        };
        game.music_game.set_looping(true);
        game.music_menu.set_looping(true);
        game.load_high_score();
        game
    }

    fn load_high_score(&mut self) {
        if let Ok(saved_score) = fs::read_to_string(HIGHSCORE_FILE) {
            if let Ok(score) = saved_score.parse::<f32>() {
                self.high_score = score;
            }
        }
    }

    fn save_high_score(&self) {
        if let Err(err) = fs::write(HIGHSCORE_FILE, format!("{}", self.high_score)) {
            eprintln!("Failed to save high score: {}", err);
        }
    }

    fn play_menu_music(&mut self) {
        switch_music(&mut self.music_game, &mut self.music_menu);
    }

    fn play_game_music(&mut self) {
        switch_music(&mut self.music_menu, &mut self.music_game);
    }

    fn spawn_ball(&mut self, balls: &mut Vec<Ball>) {
        let mut rng = rand::thread_rng();
        let position = Vector2f::new(rng.gen_range(0.0..WINDOW_SIZE_X as f32), rng.gen_range(0.0..WINDOW_SIZE_Y as f32));
        let velocity = Vector2f::new(rng.gen_range(-150.0..150.0), rng.gen_range(-150.0..150.0));
        let radius = rng.gen_range(5.0..20.0);
        let color = Color::rgb(rng.gen_range(0..=255), rng.gen_range(0..=255), rng.gen_range(0..=255));
        balls.push(Ball::new(position, velocity, radius, color));
    }
    
    fn prepare_new_game(&mut self, balls: &mut Vec<Ball>) {
        self.state = GameState::Playing;
        self.clock.restart();
        self.player.shape.set_fill_color(self.player.colors[self.player.current_color_index]);
        self.player.hp = INITIAL_HP;
        self.player.survival_time = 0.0;
        self.player.hits = 0;
        self.player.bonuses = 0;
        balls.clear();
        for _ in 0..INITIAL_BALL_COUNT {
            self.spawn_ball(balls);
        }
    }

    fn get_text_center_x(&mut self, text: &Text) -> f32 {
        (WINDOW_SIZE_X as f32 - text.global_bounds().width) / 2.0
    }

    fn run(&mut self, window: &mut RenderWindow) {
        let font = Font::from_file("font.ttf").expect("Failed to load font");
        let health_box_texture = Texture::from_file("health.png").expect("Failed to load health box texture");
        let mut balls: Vec<Ball> = Vec::new();
        let mut rng = rand::thread_rng();
        for _ in 0..INITIAL_BALL_COUNT {
            self.spawn_ball(&mut balls);
        }

        let mut health_box_clock = Clock::start();
        let mut health_boxes: Vec<HealthBox> = Vec::new();

        while window.is_open() {
            while let Some(event) = window.poll_event() {
                match event {
                    Event::Closed => window.close(),
                    Event::KeyPressed { code: Key::Space, alt: _, ctrl: _, shift: _, system: _ } => {
                        match self.state {
                            GameState::Menu => {
                                self.prepare_new_game(&mut balls);
                            }
                            GameState::Playing => { }
                            GameState::GameOver => {
                                self.state = GameState::Menu;
                            }
                        }
                    },
                    Event::KeyPressed { code: Key::Up, alt: _, ctrl: _, shift: _, system: _ } => {
                        if let GameState::Menu = self.state {
                            self.player.select_previous_color();
                        }
                    },
                    Event::KeyPressed { code: Key::Down, alt: _, ctrl: _, shift: _, system: _ } => {
                        if let GameState::Menu = self.state {
                            self.player.select_next_color();
                        }
                    }
                    _ => {}
                }
            }

            let delta_time = self.clock.restart().as_seconds();
            match self.state {
                GameState::Menu => {
                    self.play_menu_music();

                    let mut title_text = Text::new("Color Escape", &font, 36);
                    title_text.set_fill_color(Color::WHITE);
                    title_text.set_position(Vector2f::new(self.get_text_center_x(&title_text), 50.0));

                    let mut menu_text = Text::new("Press SPACE to start", &font, 36);
                    menu_text.set_fill_color(Color::WHITE);
                    menu_text.set_position(Vector2f::new(self.get_text_center_x(&menu_text), 150.0));
                    
                    let mut select_color_text = Text::new("Select Color:", &font, 24);
                    select_color_text.set_fill_color(Color::WHITE);
                    select_color_text.set_position(Vector2f::new(self.get_text_center_x(&select_color_text), 250.0));
                    
                    let mut color_circle = CircleShape::new(PLAYER_RADIUS, 30);
                    color_circle.set_fill_color(self.player.colors[self.player.current_color_index]);
                    color_circle.set_position(Vector2f::new(WINDOW_SIZE_X as f32 / 2.0, 300.0));

                    window.clear(Color::rgb(30, 30, 30));
                    window.draw(&title_text);
                    window.draw(&menu_text);
                    window.draw(&select_color_text);
                    window.draw(&color_circle);
                    window.display();
                }
                GameState::Playing => {
                    self.play_game_music();
                    self.player.update(delta_time);

                    for ball in &mut balls {
                        ball.update(delta_time);
                    }
                    balls.retain(|ball| !self.player.check_collision_with_ball(ball));

                    if health_box_clock.elapsed_time().as_seconds() >= HEALTH_BOX_SPAWN_INTERVAL {
                        let position = Vector2f::new(rng.gen_range(0.0..WINDOW_SIZE_X as f32 - HEALTH_BOX_SIZE), rng.gen_range(0.0..WINDOW_SIZE_Y as f32 - HEALTH_BOX_SIZE));
                        health_boxes.push(HealthBox::new(&health_box_texture, position));
                        health_box_clock.restart();
                    }
                    health_boxes.retain(|health_box| !self.player.check_collision_with_health_box(health_box));

                    if rng.gen_bool(0.01) {
                        self.spawn_ball(&mut balls);
                    }

                    window.clear(Color::BLACK);
                    window.draw(&self.player.shape);
                    for ball in &balls {
                        window.draw(&ball.shape);
                    }

                    for health_box in &health_boxes {
                        window.draw(&health_box.sprite);
                    }

                    let mut hp_text = Text::new(&format!("HP: {}", self.player.hp), &font, 24);
                    hp_text.set_fill_color(Color::WHITE);
                    hp_text.set_position((10.0, 10.0));
                    window.draw(&hp_text);

                    let mut time_text = Text::new(&format!("Survival Time: {:.2}", self.player.survival_time), &font, 24);
                    time_text.set_fill_color(Color::WHITE);
                    time_text.set_position((WINDOW_SIZE_X as f32 - 400.0, 10.0));
                    window.draw(&time_text);

                    let mut text = Text::new(&format!("Bonus: {:.2}", self.player.time_since_last_hit), &font, 24);
                    text.set_position((10.0, 60.0));
                    text.set_fill_color(Color::WHITE);
                    window.draw(&text);

                    window.display();

                    if self.player.hp <= 0 {
                        self.state = GameState::GameOver;
                        self.play_menu_music();
                        play_sound(&mut self.sound_game_over);
                    }
                }
                GameState::GameOver => {
                    if self.player.survival_time > self.high_score {
                        self.high_score = self.player.survival_time;
                        self.save_high_score();
                    }

                    let mut game_over_text = Text::new("Game Over!", &font, 36);
                    game_over_text.set_fill_color(Color::RED);
                    game_over_text.set_position((self.get_text_center_x(&game_over_text), WINDOW_SIZE_Y as f32 / 2.0 - 50.0));
                    window.clear(Color::BLACK);
                    window.draw(&game_over_text);

                    let mut hits_text = Text::new(&format!("Hits: {}", self.player.hits), &font, 24);
                    hits_text.set_fill_color(Color::WHITE);
                    hits_text.set_position((self.get_text_center_x(&hits_text), WINDOW_SIZE_Y as f32 / 2.0 + 50.0));
                    window.draw(&hits_text);

                    let mut bonuses_text = Text::new(&format!("Bonuses: {}", self.player.bonuses), &font, 24);
                    bonuses_text.set_fill_color(Color::WHITE);
                    bonuses_text.set_position((self.get_text_center_x(&bonuses_text), WINDOW_SIZE_Y as f32 / 2.0 + 80.0));
                    window.draw(&bonuses_text);

                    let mut your_score_text = Text::new(&format!("Your Score: {:.2}", self.player.survival_time), &font, 24);
                    your_score_text.set_fill_color(Color::WHITE);
                    your_score_text.set_position((self.get_text_center_x(&your_score_text), WINDOW_SIZE_Y as f32 / 2.0 + 110.0));
                    window.draw(&your_score_text);

                    let mut high_score_text = Text::new(&format!("High Score: {:.2}", self.high_score), &font, 24);
                    high_score_text.set_fill_color(Color::WHITE);
                    high_score_text.set_position((self.get_text_center_x(&high_score_text), WINDOW_SIZE_Y as f32 / 2.0 + 140.0));
                    window.draw(&high_score_text);

                    window.display();
                }
            }
        }
    }
}

fn play_sound(sound: &mut Music) {
    sound.stop();
    sound.play();
}

fn switch_music(old_music: &mut Music, new_music: &mut Music) {
    if new_music.status() != sfml::audio::SoundStatus::PLAYING {
        old_music.stop();
        new_music.play();
    }
}

fn main() {
    let mut window = RenderWindow::new(
        (WINDOW_SIZE_X, WINDOW_SIZE_Y),
        "Color Escape",
        Style::CLOSE,
        &ContextSettings::default(),
    );
    window.set_vertical_sync_enabled(true);
    let mut game = Game::new();
    game.run(&mut window);
}
