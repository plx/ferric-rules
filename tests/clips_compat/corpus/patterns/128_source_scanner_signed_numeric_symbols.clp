;; Invalid signed numbers and overflowing prefixes with suffixes stay whole symbols.
;; Level: boundary
;; Covers: source-scanner, explode$, symbolp
(deffunction show (?source ?text)
  (printout t ?source " " (symbolp ?source) " "
    (eq ?source (nth$ 1 (explode$ ?text))) crlf))
(defrule probe =>
  (show +12abc "+12abc") (show -12abc "-12abc")
  (show +0x10 "+0x10") (show -0x10 "-0x10")
  (show +1.abc "+1.abc") (show -3.14.15 "-3.14.15")
  (show +5f "+5f") (show -5f "-5f")
  (show 99999999999999999999x "99999999999999999999x")
  (show -99999999999999999999x "-99999999999999999999x"))
