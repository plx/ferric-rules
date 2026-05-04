(deftemplate counter (slot value (default 0)))

(defrule increment
    ?c <- (counter (value ?v))
    ?t <- (tick)
    =>
    (modify ?c (value (+ ?v 1)))
    (retract ?t))
