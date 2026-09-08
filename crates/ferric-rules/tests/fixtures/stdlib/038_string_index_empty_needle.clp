;; An empty substring is found just after the last character.
;; Level: boundary
;; Covers: str-index
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (str-index "" "abc") " " (str-index "" "") crlf))
