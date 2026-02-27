(deftemplate A (field foo (type INTEGER)))
(defrule foo (A (foo ?y&:(< ?y 3))) =>)
