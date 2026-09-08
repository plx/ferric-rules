;; Switch runs default when no case matches.
;; Level: basic
;; Covers: case, default, switch
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (switch blue
        (case red then (printout t "wrong" crlf))
        (default (printout t "default" crlf))))
