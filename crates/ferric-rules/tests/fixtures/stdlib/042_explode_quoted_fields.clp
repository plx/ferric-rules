;; Explode parses quoted strings as one field and preserves numeric types.
;; Level: interaction
;; Covers: explode$, integerp, length$, nth$, stringp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (length$ (explode$ "a \"two words\" 3")) " " (stringp (nth$ 2 (explode$ "a \"two words\" 3"))) " " (integerp (nth$ 3 (explode$ "a \"two words\" 3"))) crlf))
