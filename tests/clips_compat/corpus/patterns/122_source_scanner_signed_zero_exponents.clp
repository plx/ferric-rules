;; Signs, decimal points, and exponents preserve numeric types and signed zero.
;; Level: boundary
;; Covers: source-scanner, explode$, create$, integerp, floatp
(deffunction show (?source ?text)
  (printout t ?text ": " ?source " " (integerp ?source) " " (floatp ?source) " "
    (eq ?source (nth$ 1 (explode$ ?text))) crlf))
(defrule probe =>
  (show 0 "0") (show +0 "+0") (show -0 "-0")
  (show 0. "0.") (show +0. "+0.") (show -0. "-0.")
  (show .0 ".0") (show +.0 "+.0") (show -.0 "-.0")
  (show 0e0 "0e0") (show +0e+0 "+0e+0") (show -0e0 "-0e0")
  (show 1.e-3 "1.e-3") (show +1.e+3 "+1.e+3") (show -1.E+3 "-1.E+3")
  (show .5E1 ".5E1") (show +.5e-1 "+.5e-1") (show -.5e1 "-.5e1")
  (printout t "arithmetic: " (* 2 .5) " " (* 2 +.5) " " (* 2 -.5) crlf))
