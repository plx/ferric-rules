;; Switch runs the matching case without falling through.
;; Level: basic
;; Covers: case, default, switch
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (switch blue
        (case red then (printout t "wrong" crlf))
        (case blue then (printout t "blue" crlf))
        (case green then (printout t "wrong" crlf))
        (default (printout t "wrong" crlf))))
