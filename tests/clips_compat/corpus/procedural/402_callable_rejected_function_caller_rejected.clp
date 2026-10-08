(deffunction f (?x) (+ (nosuch ?x) 1))
(defrule r => (printout t (f 1) crlf))
