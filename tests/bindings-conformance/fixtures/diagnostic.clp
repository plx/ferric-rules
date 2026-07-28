(defrule fail
    =>
    (/ 1 0)
    (assert (must-not-run)))
