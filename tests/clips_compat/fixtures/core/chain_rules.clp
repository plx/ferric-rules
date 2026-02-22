; Multi-step rule chain
(deffacts startup (stage 1))

(defrule step-1-to-2
    ?f <- (stage 1)
    =>
    (retract ?f)
    (assert (stage 2))
    (printout t "1->2" crlf))

(defrule step-2-to-3
    ?f <- (stage 2)
    =>
    (retract ?f)
    (assert (stage 3))
    (printout t "2->3" crlf))

(defrule step-3-done
    (stage 3)
    =>
    (printout t "done" crlf))
