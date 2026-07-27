; Double-negation transitions pinned against CLIPS 6.30.
;
; The existential rule is initially false, becomes true with its first
; support, remains one Boolean match after a second support is asserted and
; the first is retracted, then becomes false after the last support is
; retracted. The single "one" line proves that the two supporting facts never
; create duplicate activations.

(deffacts startup
    (phase zero))

(defrule zero-support
    (declare (salience 100))
    ?phase <- (phase zero)
    =>
    (printout t "zero" crlf)
    (retract ?phase)
    (assert (support one)))

(defrule existential-match
    (declare (salience 80))
    (not (not (support ?value)))
    =>
    (printout t "one" crlf)
    (assert (support two))
    (assert (phase two)))

(defrule two-supports
    (declare (salience 60))
    ?phase <- (phase two)
    ?first <- (support one)
    =>
    (printout t "two" crlf)
    (retract ?phase)
    (retract ?first)
    (assert (phase partial)))

(defrule partial-support-retract
    (declare (salience 40))
    ?phase <- (phase partial)
    ?last <- (support two)
    =>
    (printout t "partial" crlf)
    (retract ?phase)
    (retract ?last)
    (assert (phase zero-again)))

(defrule zero-support-again
    (declare (salience 20))
    (phase zero-again)
    =>
    (printout t "zero-again" crlf))
