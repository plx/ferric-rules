;; Concatenating an empty STRING returns an empty STRING.
;; Level: boundary
;; Covers: str-cat, stringp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t "[" (str-cat "") "] " (stringp (str-cat "")) crlf))
