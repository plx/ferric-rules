; RH-CORE-012: empty-LHS and standalone not rules bootstrap without application facts.
(defrule empty (declare (salience 10)) => (printout t "empty" crlf) (assert (ready)))
(defrule absent (ready) (not (blocker)) => (printout t "absent" crlf) (assert (result yes)))
