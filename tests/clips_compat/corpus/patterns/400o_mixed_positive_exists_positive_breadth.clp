(defrule seed (declare (salience 100)) => (assert (a) (b) (c) (item)))
(defrule r1 (a) (item) => (printout t r1 crlf))
(defrule r2 (b) (exists (item)) => (printout t r2 crlf))
(defrule r3 (c) (item) => (printout t r3 crlf))
