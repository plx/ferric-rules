(deftemplate left-item (slot value))
(deftemplate right-item (slot value))
(deffacts seed
  (left-item (value a)) (left-item (value b))
  (right-item (value x)) (right-item (value y)))
(defrule probe =>
  (do-for-all-facts ((?a left-item) (?b right-item)) TRUE
    (printout t ?a:value ":" ?b:value crlf)
    (if (and (eq ?a:value a) (eq ?b:value x)) then
      (do-for-fact ((?later right-item)) (eq ?later:value y) (retract ?later))
      (assert (right-item (value y))))))
