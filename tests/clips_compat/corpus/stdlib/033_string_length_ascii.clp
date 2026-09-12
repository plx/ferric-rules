;; String length handles an empty string and ASCII characters.
;; Level: basic
;; Covers: str-length
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (str-length "") " " (str-length "abc") crlf))
