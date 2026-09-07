; Empty template patterns remain unconstrained by the template slot count.
(deftemplate row (slot a) (slot b))
(deffacts input (row (a first) (b second)))
(defrule observe (row) => (printout t "template" crlf))
