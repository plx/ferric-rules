; FR-RETE-003 stage 2: rules must activate from the existing working memory.
(defrule late-single
    (declare (salience 30))
    (seed alice)
    =>
    (printout t "single" crlf))

(defrule late-join
    (declare (salience 20))
    (left ?value)
    (right ?value)
    =>
    (printout t "join " ?value crlf))

(defrule late-negative-exists
    (declare (salience 10))
    (seed ?value)
    (not (blocked ?value))
    (exists (ready ?value))
    =>
    (printout t "state " ?value crlf))
