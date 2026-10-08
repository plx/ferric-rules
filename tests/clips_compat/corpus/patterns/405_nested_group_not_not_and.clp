(deffacts d (a) (b))
(defrule hit (declare (salience 10)) (not (not (and (a) (b)))) => (printout t hit crlf))
(defrule negated-members (not (not (and (not (c)) (not (d))))) => (printout t negated-members crlf))
(defrule miss (not (not (and (a) (c)))) => (printout t miss crlf))
