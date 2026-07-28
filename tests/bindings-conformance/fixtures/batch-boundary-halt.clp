(deffacts startup
    (counter 0))

(defrule advance
    ?counter <- (counter ?value&:(< ?value 101))
    =>
    (retract ?counter)
    (assert (counter (+ ?value 1)))
    (if (= ?value 99) then
        (halt)))
