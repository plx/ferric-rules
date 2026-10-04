(defrule seed (declare (salience 100)) => (assert (a) (b) (c) (item)))
(defrule r1 (a) (exists (item)) => (printout t r1 crlf))
(defrule r2 (b) (item) => (printout t r2 crlf))
(defrule r3 (c) (exists (item)) => (printout t r3 crlf))
