
(defrule probe =>
;; NaN spelling varies by platform; implode$ must agree with scalar spelling.
(bind ?nan (sin 1.0e309))
(bind ?spelling (str-cat ?nan))
(printout t "["
  (and (or (eq ?spelling "nan.0") (eq ?spelling "-nan.0"))
       (eq (implode$ (create$ ?nan 1.0e309 -1.0e309))
           (str-cat ?spelling " inf.0 -inf.0"))) "]" crlf)
)
