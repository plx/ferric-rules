(deffacts seed (p 1) (p 2) (p 3))
(defrule change (declare (salience 10)) => (printout t (set-strategy breadth) " " (get-strategy) crlf))
(defrule show (p ?n) => (printout t ?n crlf))
