(deftemplate result
  (slot value))

(defrule produce-unicode
  =>
  (assert (result (value "café"))))
