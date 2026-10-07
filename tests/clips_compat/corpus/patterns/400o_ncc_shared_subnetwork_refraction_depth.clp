(deffacts seed (b) (c) (phase 1))
(defrule helper (a) (b) (c) => (printout t helper crlf))
(defrule nested (not (and (a) (not (and (b) (c))))) => (printout t nested crlf))
(defrule add-a (declare (salience -10)) ?p <- (phase 1) => (retract ?p) (printout t add-a crlf) (assert (a)))
