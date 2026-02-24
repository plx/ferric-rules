(deftemplate A (slot val))
(deftemplate B)
(deftemplate C)
(deftemplate D)
(deffacts infinite_setup
  (A (val 1)))
(defrule infinite_rule
  (logical
    (A (val ?val))
    (not (and (B) (C)))
    (test (eq ?val 1))
    (not (D)))
  =>
  (assert (D)))
