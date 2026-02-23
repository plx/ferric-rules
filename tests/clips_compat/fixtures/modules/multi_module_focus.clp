; Focus stack drives module execution order.
; MAIN fires first (default focus), then focus is pushed to A, then B.
(defmodule A)
(defmodule B)

(deffacts MAIN::startup (go))

(defrule MAIN::start
    (go)
    =>
    (focus A)
    (printout t "MAIN" crlf))

(defrule A::do-a
    (initial-fact)
    =>
    (focus B)
    (printout t "A" crlf))

(defrule B::do-b
    (initial-fact)
    =>
    (printout t "B" crlf))
