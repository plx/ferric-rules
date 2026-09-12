;; String comparison returns negative, zero, and positive ordering values.
;; Level: basic
;; Covers: str-compare
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (str-compare "a" "b") " " (str-compare "same" "same") " " (str-compare z a) crlf))
